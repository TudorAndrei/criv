---
id: ADR-0092
kind: decision
title: Transactional Live Watch Generations
status: accepted
date: 2026-08-11
supersedes:
  - ADR-0042
governs:
  - src/watch.rs
  - src/refresh.rs
  - src/source/index.rs
  - src/vault.rs
---

# Transactional Live Watch Generations

## Context

[[0042-shared-source-index-lifecycle|ADR-0042]] made the source index the
authoritative source catalog. It also required a live watcher to reuse one
catalog lifecycle for source search and graph construction. That decision
assumed that configuration and watched paths stayed fixed for the process
lifetime.

The current implementation splits live ownership. `src/watch.rs#fn:run` loads
configuration and registers the docs watch once.
`src/refresh.rs#type:RefreshSession` owns one source lifecycle.
`src/vault.rs#type:index/member:load_incremental_with_source_catalog` loads
configuration again during each refresh. A later refresh can therefore use new
configuration with the old source catalog and old watched paths.

`src/source_index.rs#type:SourceIndexLifecycle` also builds its source scan plan
once. A configured source root that does not exist at startup has no picker or
recovery watch. If it appears later, the active lifecycle cannot observe it.
The process does not watch `criv.toml`, so a valid change does not start a
refresh and a repaired invalid change does not cause recovery by itself.

## Decision

Retain the single-catalog rule from ADR-0042. A one-shot command creates one
non-watching source lifecycle for one vault load. A live watch has one
authoritative source lifecycle and source catalog at a time. Source search,
source graph construction, and State generation consume that same catalog.

Define an **active watch generation** as one accepted configuration, one
`RefreshSession`, one source lifecycle, one docs root, and one active watch
set. A `LiveWatchSession` module in `src/watch.rs` owns the active generation.
The event loop uses the module interface and does not own these values
separately. Vault refresh receives the generation configuration; it does not
reload `criv.toml` independently.

A create, change, rename, or deletion of `criv.toml` starts candidate work.
Deletion selects the default configuration. A change in normalized
configuration builds a complete candidate generation from one file snapshot.
The candidate contains its configuration, source lifecycle, source catalog,
docs root, and watch set. Run one full refresh with those values. Replace the
active generation only after all candidate work succeeds. Candidate setup can
temporarily allocate another source lifecycle, but it is never authoritative.

Ordinary content changes in the active docs or source roots cause a normal
refresh. They do not replace the generation. A burst of configuration and
content events uses the final debounced filesystem contents. An event that
arrives during candidate work remains queued and causes another observation
after the candidate completes.

An absent configured source directory or explicit file root is valid and
contributes no source files. Watch its nearest existing ancestor. Build a new
candidate when the root appears, disappears, or changes between a file and a
directory. Apply the same topology check to a root that existed when the
generation started.

The configured docs root must exist and be a directory. If it is absent,
reject the candidate, keep the active generation, and watch the nearest
existing ancestor. Retry candidate work when the docs root appears. Do not
publish an empty vault because the configured docs root is temporarily absent.

An initial candidate failure exits with status 1. After startup, a candidate
failure keeps the process alive, keeps the active generation, and keeps the
last successful State required by
[[0073-effective-adr-governance-and-source-reconciliation|ADR-0073]]. Suspend
all State publication until recovery. A recovery attempt runs one full
candidate refresh before publication resumes.

Keep a small recovery watch set for a rejected candidate. It watches
`criv.toml`, the candidate docs path when it exists, and the nearest existing
ancestors of missing docs or source roots. Do not retain a rejected candidate
source lifecycle or catalog. Each recovery event builds a new candidate from
disk.

Report a changed failure state once on standard error as
`criv watch: reconfiguration failed: <cause>; keeping last successful State`.
Report `criv watch: reconfiguration recovered` after success. Do not add a
State field or a status file. A watcher or source adapter failure suspends
publication and retries adapter creation once per second. A disconnected event
channel is fatal and exits with status 1.

Live replacement is automatic. Do not add a command flag, restart command, or
status file. `criv watch --once` keeps its existing one-shot behavior.

Required verification compares every successful live generation with a fresh
one-shot build from the same configuration and files. Tests cover a
configuration-only event; changes to `source.roots`, `source.exclude`,
`index.source`, and `vault.docs`; missing and replaced source roots; a missing
docs root; invalid configuration; frozen publication; recovery watches;
adapter restart; event-channel disconnection; and event bursts. Portable state
tests run on all platforms. Real watcher integration tests run in the required
Linux and Windows lanes. This decision does not add a hosted macOS lane.

## Consequences

Every published live refresh uses one configuration, one source catalog, and
one watch topology from the same successful generation. A new configuration
cannot become partly active.

Candidate setup can briefly use more resources because the old generation
stays available until replacement succeeds. The extra lifecycle is staged,
not authoritative, and is removed after success or failure.

An invalid configuration or failed adapter no longer needs an unrelated file
change for recovery. State can stay unchanged while the process is in a clear
failure state. The next successful full refresh includes all changes that
occurred during the failure.

The exact multi-file State publication and rollback order is not part of this
decision. It remains a separate publication contract.
