#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { basename, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { parseArgs } from "node:util";
import { fileURLToPath } from "node:url";

const OUTPUT_SCHEMA = "criv.state-wasm-baseline.v1";
const OPERATIONS = [
  "cold_load_and_initial_projections",
  "initial_projections_after_load",
  "lookup_present",
  "lookup_missing",
  "selector_empty",
  "selector_exact",
  "selector_suffix",
  "selector_missing",
];

const definitions = {
  cold_load_and_initial_projections:
    "load the packaged Wasm module, read the complete State, and run the four initial editor projections",
  initial_projections_after_load:
    "run validated_state, summarize_state, source_entries, and graph_nodes after module and file load",
  lookup_present: "look up one existing graph-node ID after module and file load",
  lookup_missing: "look up one absent graph-node ID after module and file load",
  selector_empty: "request source selectors with an empty query after module and file load",
  selector_exact: "request source selectors with one exact source path after module and file load",
  selector_suffix: "request source selectors with one source basename after module and file load",
  selector_missing: "request source selectors with one absent query after module and file load",
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

function runInitialProjections(wasm, raw) {
  const values = [
    wasm.validated_state(raw),
    wasm.summarize_state(raw),
    wasm.source_entries(raw),
    wasm.graph_nodes(raw),
  ];
  if (values.length !== 4) throw new Error("initial projection count changed");
}

function operationInputs(wasm, raw) {
  const nodes = wasm.graph_nodes(raw);
  const sources = wasm.source_entries(raw);
  const presentNode = nodes[0]?.id ?? "__criv_missing_node__";
  const exactSource = sources[0]?.path ?? "__criv_missing_source__";
  return {
    presentNode,
    exactSource,
    suffixSource: basename(exactSource),
  };
}

function runOperation(wasm, raw, operation, inputs) {
  switch (operation) {
    case "initial_projections_after_load":
    case "cold_load_and_initial_projections":
      return runInitialProjections(wasm, raw);
    case "lookup_present":
      return wasm.lookup_graph_node(raw, inputs.presentNode);
    case "lookup_missing":
      return wasm.lookup_graph_node(raw, "__criv_missing_node__");
    case "selector_empty":
      return wasm.suggest_source_selectors(raw, "", 20);
    case "selector_exact":
      return wasm.suggest_source_selectors(raw, inputs.exactSource, 20);
    case "selector_suffix":
      return wasm.suggest_source_selectors(raw, inputs.suffixSource, 20);
    case "selector_missing":
      return wasm.suggest_source_selectors(raw, "__criv_missing_selector__", 20);
    default:
      throw new Error(`unsupported operation: ${operation}`);
  }
}

function worker(values) {
  const state = required(values.state, "state");
  const packageRoot = required(values.package, "package");
  const operation = values.operation;
  if (!OPERATIONS.includes(operation)) throw new Error(`invalid --operation: ${operation}`);

  let wasm;
  let raw;
  let inputs;
  let start;
  if (operation === "cold_load_and_initial_projections") {
    start = performance.now();
    wasm = loadPackage(packageRoot);
    raw = readFileSync(state, "utf8");
    runInitialProjections(wasm, raw);
  } else {
    wasm = loadPackage(packageRoot);
    raw = readFileSync(state, "utf8");
    inputs = operationInputs(wasm, raw);
    start = performance.now();
    runOperation(wasm, raw, operation, inputs);
  }
  const seconds = (performance.now() - start) / 1000;
  process.stdout.write(
    `${JSON.stringify({ seconds, max_rss_bytes: process.resourceUsage().maxRSS * 1024 })}\n`,
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
  const result = spawnSync(
    process.execPath,
    [
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
          : "fresh process with module and State loaded before the timed operation, after one untimed process warm-up",
      raw,
      timing: timing(raw),
      peak_rss: memory(raw),
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

try {
  const values = options();
  if (values.worker) worker(values);
  else parent(values);
} catch (error) {
  process.stderr.write(`measure-state-wasm: ${error instanceof Error ? error.message : error}\n`);
  process.exitCode = 1;
}
