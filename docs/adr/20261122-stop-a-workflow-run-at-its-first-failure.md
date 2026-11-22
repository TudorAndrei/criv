---
id: ADR-20261122-stop-a-workflow-run-at-its-first-failure
kind: decision
title: Stop a workflow run at its first failure
status: accepted
date: 2026-11-22
governs:
  - .github/workflows/ci.yml
  - .github/actions/stop-run-on-failure/**
---

# Stop a workflow run at its first failure

## Context

The CI workflow runs many parallel jobs on Linux, macOS, and Windows. When one
job fails, the other jobs continue until they finish, but the run cannot pass.
The fix for the failure is a new push, which starts a full new run. Before this
decision, only pull request runs cancelled an older run of the same pull
request. A push run continued next to the new run.

## Decision

A CI run stops at its first failure. Each CI job, except the "Repository checks"
summary, ends with a step that runs only on failure. The step uses the
`.github/actions/stop-run-on-failure` action, which cancels the complete
workflow run with `gh run cancel`. Each of these jobs has the `actions: write`
permission for this step. The workflow level keeps `contents: read`.

A new run cancels the older run in the same concurrency group, for pull
requests and for pushes to `main`.

## Consequences

A failed run shows one failed job and cancelled jobs. To see whether a
cancelled job also has a failure, fix the first failure and push again.

A `main` push that a newer push replaces gets no CI result. The automatic
release uses only the newest successful `main` run, so this has no effect on
releases.

When the token cannot cancel the run, for example in a pull request from a
fork, the step reports the problem and the other jobs continue.
