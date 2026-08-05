#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { basename, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { parseArgs } from "node:util";
import { fileURLToPath } from "node:url";

const SCHEMA = "criv.state-store-wasm-candidate.v1";
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

function options() {
  return parseArgs({
    options: {
      candidate: { type: "string" },
      store: { type: "string" },
      package: { type: "string" },
      samples: { type: "string", default: "5" },
      "allow-low-samples": { type: "boolean", default: false },
      worker: { type: "boolean", default: false },
      operation: { type: "string" },
    },
    strict: true,
  }).values;
}

function required(value, name) {
  if (!value) throw new Error(`--${name} is required`);
  return value;
}

function loadStore(values) {
  const packageRoot = resolve(required(values.package, "package"));
  const wasm = createRequire(import.meta.url)(packageRoot);
  const bytes = readFileSync(resolve(required(values.store, "store")));
  return new wasm.CandidateStore(required(values.candidate, "candidate"), bytes);
}

function projections(store) {
  return [store.validatedState(), store.summary(), store.sourceEntries(), store.graphNodes()];
}

function inputs(store) {
  const node = store.graphNodes()[0]?.id ?? "__criv_missing_node__";
  const source = store.sourceEntries()[0]?.path ?? "__criv_missing_source__";
  return { node, source, suffix: basename(source) };
}

function runOperation(store, operation, values) {
  switch (operation) {
    case "cold_load_and_initial_projections":
    case "initial_projections_after_load":
      return projections(store);
    case "lookup_present":
      return store.lookup(values.node);
    case "lookup_missing":
      return store.lookup("__criv_missing_node__");
    case "selector_empty":
      return store.selectors("", 20);
    case "selector_exact":
      return store.selectors(values.source, 20);
    case "selector_suffix":
      return store.selectors(values.suffix, 20);
    case "selector_missing":
      return store.selectors("__criv_missing_selector__", 20);
    default:
      throw new Error(`unsupported operation: ${operation}`);
  }
}

function worker(values) {
  const operation = required(values.operation, "operation");
  if (!OPERATIONS.includes(operation)) throw new Error(`invalid operation: ${operation}`);
  let store;
  let operationInputs;
  let start;
  if (operation === "cold_load_and_initial_projections") {
    start = performance.now();
    store = loadStore(values);
    projections(store);
  } else {
    store = loadStore(values);
    operationInputs = inputs(store);
    start = performance.now();
    runOperation(store, operation, operationInputs);
  }
  process.stdout.write(
    `${JSON.stringify({
      seconds: (performance.now() - start) / 1000,
      max_rss_bytes: process.resourceUsage().maxRSS * 1024,
    })}\n`,
  );
}

function median(values) {
  const ordered = [...values].sort((left, right) => left - right);
  const middle = Math.floor(ordered.length / 2);
  return ordered.length % 2 === 1 ? ordered[middle] : (ordered[middle - 1] + ordered[middle]) / 2;
}

function summarize(raw) {
  const seconds = raw.map((sample) => sample.seconds);
  const center = median(seconds);
  const memory = raw.map((sample) => sample.max_rss_bytes);
  return {
    raw,
    timing: {
      samples: raw.length,
      minimum_seconds: Math.min(...seconds),
      median_seconds: center,
      maximum_seconds: Math.max(...seconds),
      median_absolute_deviation_seconds: median(seconds.map((value) => Math.abs(value - center))),
    },
    peak_rss: {
      unit: "bytes",
      minimum: Math.min(...memory),
      median: median(memory),
      maximum: Math.max(...memory),
    },
  };
}

function runWorker(values, operation) {
  const args = [
    fileURLToPath(import.meta.url),
    "--worker",
    "--candidate",
    values.candidate,
    "--store",
    values.store,
    "--package",
    values.package,
    "--operation",
    operation,
  ];
  const result = spawnSync(process.execPath, args, { encoding: "utf8" });
  if (result.status !== 0) throw new Error(`${operation} worker failed: ${result.stderr.trim()}`);
  return JSON.parse(result.stdout);
}

function wasmModule(packageRoot) {
  const name = readdirSync(packageRoot)
    .filter((entry) => entry.endsWith(".wasm"))
    .sort()[0];
  if (!name) throw new Error(`no .wasm module found in ${packageRoot}`);
  return resolve(packageRoot, name);
}

function parent(values) {
  values.candidate = required(values.candidate, "candidate");
  values.store = resolve(required(values.store, "store"));
  values.package = resolve(required(values.package, "package"));
  const samples = Number.parseInt(values.samples, 10);
  if (!Number.isInteger(samples) || samples < 1 || (samples < 3 && !values["allow-low-samples"])) {
    throw new Error("samples must be at least 3; use --allow-low-samples only in tests");
  }
  const operations = {};
  for (const operation of OPERATIONS) {
    runWorker(values, operation);
    const raw = [];
    for (let sample = 0; sample < samples; sample += 1) raw.push(runWorker(values, operation));
    operations[operation] = summarize(raw);
  }
  const module = wasmModule(values.package);
  process.stdout.write(
    `${JSON.stringify({
      schema: SCHEMA,
      candidate: values.candidate,
      samples,
      store_bytes: statSync(values.store).size,
      wasm_module: module,
      wasm_module_bytes: statSync(module).size,
      operations,
    })}\n`,
  );
}

try {
  const values = options();
  if (values.worker) worker(values);
  else parent(values);
} catch (error) {
  process.stderr.write(`measure-state-candidate-wasm: ${error instanceof Error ? error.message : error}\n`);
  process.exitCode = 1;
}
