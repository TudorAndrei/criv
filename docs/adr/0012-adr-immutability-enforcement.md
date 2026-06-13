---
id: ADR-0012
kind: decision
title: ADR Immutability Enforcement
status: accepted
date: 2026-05-13
governs:
  - src/enforce.rs
  - src/check.rs
---

# ADR Immutability Enforcement

## Context

Accepted ADRs describe decisions that other code and documentation can rely on.
The repository already treats follow-up decisions as new ADRs instead of edits
to accepted records, as shown by [[0011-embed-runtime-skill-templates-as-assets|ADR-0011]] referencing [[0010-criv-init-installs-agent-runtime-skills|ADR-0010]].

That convention was documented but not enforced. A contributor could still
modify, delete, or rename an existing ADR file, and the supersession validator
expected reciprocal `superseded_by` edits on old ADRs. That made append-only ADR
history depend on discipline rather than criv enforcement.

## Decision

`criv enforce` rejects modifications, deletions, and renames of existing ADR
files under `docs/adr/`, excluding `docs/adr/README.md`. New ADR files are
allowed, including files Git reports as copies, because they do not mutate the
original decision record.

Local commit and push enforcement use Git name-status output for staged or
pushed changes. CI enforcement remains a full validation and policy pass, and
also checks ADR immutability when a comparison base is available through
`CRIV_BASE_REF` or `GITHUB_BASE_REF`.

Supersession now uses forward-only metadata: a new ADR may list older decisions
in `supersedes`, while older ADRs do not need to be edited with
`superseded_by`. `superseded_by` remains understood for existing metadata, but
it is no longer required for consistency.

## Consequences

Decision history becomes append-only in the same enforcement path as source
policy checks. Changing an accepted decision requires creating a new ADR that
references the older one, not rewriting the existing file.

CI systems that want immutable ADR checks across already-committed branch
changes should provide a base ref. Without a base ref, local CI enforcement can
still catch working-tree ADR edits but cannot infer branch history by itself.
