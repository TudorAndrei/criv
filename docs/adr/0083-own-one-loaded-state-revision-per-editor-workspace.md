---
id: ADR-0083
kind: decision
title: Own One Loaded State Revision Per Editor Workspace
status: accepted
date: 2026-08-06
governs:
  - crates/criv-wasm/src/lib.rs
  - extensions/vscode-criv/src/**
  - .obsidian/plugins/criv/src/**
  - scripts/performance/measure-state-wasm.mjs
---

# Own One Loaded State Revision Per Editor Workspace

## Context

[[0071-make-wasm-editor-projections-canonical|ADR-0071]] makes packaged Wasm
the only State decode and projection path for the editor companions. The
bridge still accepted complete State JSON for each summary, source, node,
lookup, and selector request. One editor refresh could decode the same State
several times. Interactive queries also repeated this work.

The host must keep an old revision available while a new file is read. It must
also reject late refresh results. A late result can otherwise replace newer
State or keep Wasm memory after a workspace closes.

The companions have different change signals. VS Code can watch the State
file. Obsidian does not give a supported event for a hidden `.criv` file.

## Decision

Create one canonical Wasm `LoadedState` for each open VS Code workspace or
Obsidian vault. `loadState(raw)` decodes and validates one complete State
revision. It returns a host handle with these synchronous operations:

- `initialProjections()` returns the complete validated State, summary,
  sources, and nodes in one batch;
- `lookupNode(target)` uses the prepared node index;
- `suggestSelectors(query, limit)` uses prepared source and node data;
- `dispose()` releases the Wasm revision.

The host calls `initialProjections()` once and caches its result. It does not
send raw State to later operations. Wasm releases the full decoded Rust
envelope after it creates the initial batch. It keeps only the prepared
summary, safe sources, editor nodes, lookup index, selector entries, and empty
query order.

A refresh builds a complete candidate before it changes the active revision.
The latest-started refresh owns the result. While it runs, the host keeps the
old revision available. A successful latest refresh swaps the candidate and
then disposes the old revision. A late candidate is disposed and cannot
publish data. A failed latest refresh clears and disposes the old revision and
reports the error.

The host also disposes the active revision when the State file is missing or
invalid, when the State path or workspace changes, and when the companion
unloads. All public handle methods after disposal fail with
`criv-loaded-state-disposed`. Invalid JSON and an unsupported schema remain
different errors.

VS Code refreshes after State create, change, or delete events, a manual
refresh, and a workspace-folder change. Obsidian refreshes on initial load, a
manual refresh, and a State-path change. It also compares State file modified
time and size every two seconds. Obsidian leaf and metadata events only update
views. They do not decode State. Both companions dispose their revision during
shutdown.

TypeScript owns file I/O, invalidation, refresh ordering, view state, and
lifecycle cleanup. It does not parse State or implement fallback projections.
Wasm owns validation, projection, lookup, selector ranking, and prepared query
data.

## Performance evidence

The matched old and new runs used the same workload files, release Wasm build,
machine, Node process harness, cache state, and five-sample median. The runs
preserved raw samples and median absolute deviation outside the repository, as
required by [[0072-keep-performance-observation-outside-core|ADR-0072]]. Times
below are median milliseconds. Memory is median process peak MB.

| Operation | Small old | Small new | Medium old | Medium new |
| --- | ---: | ---: | ---: | ---: |
| Cold load and initial batch | 17.291 | 10.645 | 86.878 | 31.843 |
| Initial batch after load | 10.056 | 0.087 | 82.326 | 0.075 |
| Existing node lookup | 1.728 | 0.088 | 8.397 | 0.093 |
| Missing node lookup | 1.834 | 0.081 | 8.867 | 0.088 |
| Empty selector query | 2.492 | 0.238 | 10.372 | 0.249 |
| Exact selector query | 2.734 | 0.681 | 10.487 | 1.416 |
| Suffix selector query | 2.564 | 0.667 | 10.561 | 1.356 |
| Missing selector query | 2.641 | 0.562 | 10.957 | 1.276 |

The first-batch ratios are 0.9% and 0.1% of the old medians. Lookup ratios are
at most 5.1% for the small workload and 1.1% for the medium workload. Selector
ratios are at most 26.0% and 13.5%. These results pass the 40%, 25%, and 50%
goals in [[0081-require-material-state-store-performance-gains|ADR-0081]].

Across the measured operations, the new peak memory is 95.9% to 101.0% of the
old value for the small workload and 90.7% to 106.1% for the medium workload.
It passes the 110% limit. Twenty warm load, project, and dispose cycles have a
maximum-to-first memory ratio of 1.008 for the small workload and 1.002 for the
medium workload. They show no continued growth.

## Consequences

One State revision is decoded once for an editor refresh. Initial views share
one batch, and interactive node and selector operations reuse prepared data.

The host has explicit ownership work. Every replacement, error, path change,
workspace change, and shutdown path must dispose a revision. Race tests must
use distinct fake revisions and prove that late results cannot become active.

Obsidian can detect hidden State changes without an unsupported vault event,
but it performs one file status check every two seconds. A file change can take
up to two seconds to appear without a manual refresh.
