---
id: ADR-20261122-remove-the-performance-tooling
kind: decision
title: Remove the performance tooling
status: accepted
date: 2026-11-22
supersedes:
  - ADR-0065
  - ADR-0081
  - ADR-0083
  - ADR-0103
  - ADR-0121
  - ADR-0153
  - ADR-0154
  - ADR-0171
  - ADR-0173
  - ADR-0184
governs:
  - src/*.rs
  - src/state/snapshots.rs
  - src/init/templates.rs
  - src/source/graph.rs
  - Cargo.toml
  - Cargo.lock
  - crates/**/Cargo.toml
  - crates/criv-state-wire/src/lib.rs
  - crates/criv-wasm/src/lib.rs
  - crates/criv-wasm/src/source.rs
  - criv.toml
  - .gitignore
  - mise.toml
  - mise.lock
  - hk.pkl
  - package.json
  - package-lock.json
  - packages/**/package.json
  - packages/criv-editor-state/src/*.ts
  - .obsidian/plugins/criv/package.json
  - .obsidian/plugins/criv/src/**
  - extensions/vscode-criv/package.json
  - extensions/vscode-criv/src/**
  - extensions/vscode-criv/README.md
  - fixtures/editor/source-target-lookup.v1.json
  - .github/workflows/**
  - .github/actions/setup-build-tools/**
  - scripts/lint-workflows.sh
  - scripts/prepare-docs-site.mjs
  - scripts/release-auto.sh
  - tests/ci-cache.sh
  - tests/hosted_release_scripts.sh
  - .config/wt.toml
  - .worktreeinclude
policy:
  patterns:
    - id: no-repeated-reverse-query-scans
      language: rust
      rule: |
        any:
          - pattern: cited_by($VAULT, $ID)
          - pattern: orphan_docs($VAULT)
          - pattern: references($VAULT, $SYMBOL)
          - pattern: nodes($VAULT, $KIND, $WITHOUT_DOCS)
      message: Reverse queries must use the request-local NoteCitationIndex or SourceReferenceIndex instead of repeated vault scans.
targets:
  symbols:
    - src/query.rs#type:NoteCitationIndex
    - src/query.rs#type:SourceReferenceIndex
    - src/query.rs#type:QueryCommand/member:answer
    - src/query.rs#fn:collect_source_reference_keys
---

# Remove the performance tooling

## Context

The repository has a performance harness. It has a crate in
`scripts/performance`, workload fixtures in `fixtures/performance`, the
`mise run perf` and `mise run perf-container` tasks, the "Performance notes"
workflow, and a performance smoke job in CI.
[[0069-repeatable-two-tier-performance-evidence|ADR-0069]],
[[0070-publish-push-performance-evidence-as-git-notes|ADR-0070]],
[[0072-keep-performance-observation-outside-core|ADR-0072]], and
[[0173-observe-discovery-through-a-feature-gated-selection-api|ADR-0173]] made
this harness and the `selection` Cargo feature for it.
[[0081-require-material-state-store-performance-gains|ADR-0081]] and
[[0121-separate-performance-evidence-from-automatic-release|ADR-0121]] use its
evidence.

Nobody uses the evidence to make a decision. The harness runs on each push,
and each criv behavior change must also change the harness. After the dated
ADR ids, the harness workloads failed until the generator was changed. The
cost is larger than the value.

## Decision

Remove the performance tooling completely:

- the `scripts/performance` crate, the state-store prototype, and the
  container test;
- `fixtures/performance` and `tests/fixtures/performance`;
- `scripts/measure-performance.sh` and the `tests/performance_*.sh` scripts;
- the "Performance notes" workflow and the CI performance smoke job;
- the `perf` and `perf-container` mise tasks;
- the `selection` Cargo feature and `src/selection.rs`;
- `docs/performance.md`.

CI keeps `tests/ci-cache.sh` and `tests/hosted_release_scripts.sh` in a
"Release tooling smoke tests" job. These scripts test the release and cache
setup, not performance.

The decisions of ADR-0081, ADR-0121, and ADR-0173 end. A change no longer needs
performance evidence.

The decisions of [[0065-shared-compiled-policy-scan-plans|ADR-0065]],
[[0083-own-one-loaded-state-revision-per-editor-workspace|ADR-0083]],
[[0103-required-repository-self-governance|ADR-0103]],
[[0153-retain-workspace-lint-policy-in-standard-form|ADR-0153]],
[[0154-cache-hosted-toolchains-with-mise-and-compiler-results-with-mbx|ADR-0154]],
[[0171-retain-state-wire-and-elixir-rules-after-the-editor-state-move|ADR-0171]],
and [[0184-give-each-query-reverse-index-scope-its-own-type|ADR-0184]] stay in
force without their performance parts. This decision holds their remaining
`governs` scope. It also holds the ADR-0184 targets and its policy,
[[match:ADR-20261122-remove-the-performance-tooling/no-repeated-reverse-query-scans]].

## Consequences

The workspace has two crates: `criv-state-wire` and `criv-wasm`. CI has one
less workflow and one less job.

criv has no performance baseline. If a speed problem occurs, measure it for
that problem and record the decision in a new ADR.
