---
id: ADR-0070
kind: decision
title: Publish Push Performance Evidence As Git Notes
status: accepted
date: 2026-08-03
governs:
  - .github/workflows/*.yml
  - scripts/performance/**
targets:
  symbols:
    - scripts/measure-performance.sh
    - scripts/performance/src/main.rs
---

# Publish Push Performance Evidence As Git Notes

## Context

[[0069-repeatable-two-tier-performance-evidence|ADR-0069]] makes canonical
performance evidence reproducible and deliberately keeps it outside validation
gates. That evidence is still an explicitly invoked local artifact, so a pushed
commit does not carry a durable pointer to its own measurements.

Running the full harness in `pre-push` would slow local delivery, make a
machine-sensitive result part of a correctness gate, and still would not
publish a Git note with the branch update. Notes live in separate Git refs;
ordinary branch pushes do not transfer them implicitly. A nested push from a
hook would also complicate failure and recursion semantics.

GitHub Actions can run the canonical harness asynchronously after a repository
push. Git notes supplement a commit without rewriting it, and a dedicated notes
ref can hold structured evidence without mixing machine-generated data into the
source tree. Raw evidence can remain a bounded workflow artifact while the note
keeps the durable summary.

## Decision

Add a standalone `Performance notes` workflow for repository `push` events and
manual recovery runs. It is non-gating: do not add it to the hosted validation
aggregate, hk hooks, or branch-protection requirements. Run on `ubuntu-24.04`
with the repository Rust toolchain, build the explicit release binary, and run
the two canonical host workloads with five samples and structured measurement
enabled. Do not require Docker or Testcontainers in this lane.

Upload the complete unique performance result directory as a workflow artifact
with 30-day retention, including failed evidence when the harness creates it.
After a successful harness run, render a deterministic JSON note containing:

- schema, commit, pushed ref, workflow run/attempt URL, and artifact name;
- binary, machine, workload, profile, sample, and measurement identities;
- timing summaries for every workload/case/cache tuple; and
- one deterministic counter map for every successful workload/case tuple.

Fail note rendering when successful samples in one tuple disagree on their
deterministic counters. The raw JSONL remains authoritative for individual
samples and the Git note remains a compact index and summary.

Publish notes to `refs/notes/criv-performance` with the workflow-scoped
`GITHUB_TOKEN` and only `contents: write` permission. A token-authenticated
notes-ref update must not recursively start another push workflow. Never use a
personal token for this automation.

Treat one commit as one note identity. A later successful manual recovery or
second pushed ref for the same commit replaces that commit's note. Concurrent
push workflows may race on the shared notes ref, so publication must fetch the
latest remote notes ref, reapply the target note, and retry a bounded number of
non-fast-forward failures. Never force-push the remote notes ref.

Document explicit consumption:

```sh
git fetch origin refs/notes/criv-performance:refs/notes/criv-performance
git log --notes=criv-performance
git notes --ref=criv-performance show <commit>
```

Do not configure contributor remotes or `notes.displayRef` automatically.

## Consequences

Every successful repository push receives durable, commit-addressed performance
metadata without delaying the developer's push or changing the commit object.
The workflow can fail independently when a commit does not build, a canonical
case fails, counters become nondeterministic, artifact upload fails, or the
notes ref cannot be published; those failures are visible automation failures
but not correctness gates.

The notes ref and its JSON blobs grow with measured commits. Raw artifacts
expire after 30 days, while summaries stay in Git until deliberately pruned.
Hosted-runner timings remain supporting evidence tied to their recorded machine
identity, not universal performance claims.

The workflow consumes hosted build time on every push. Avoiding Docker and
reusing Cargo caches bounds that cost, while retaining five samples preserves
the evidence floor established by ADR-0069.
