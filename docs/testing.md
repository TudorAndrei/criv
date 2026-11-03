---
id: testing
kind: doc
title: Executable Feature Specifications
---

# Executable Feature Specifications

Declare user-facing features and flows in Gherkin `.feature` files under
`tests/features/`. [Cucumber Rust](https://cucumber-rs.github.io/cucumber/)
executes them against the real CLI using Rust steps in `tests/acceptance.rs`.
The [Gherkin reference](https://cucumber.io/docs/gherkin/reference/) describes
`Feature`, `Scenario`, `Given`, `When`, `Then`, and scenario outlines.

## Features and decisions

Features specify observable behavior: the user's starting situation, action,
and outcome. ADRs record architectural choices, trade-offs, and rationale.
An architectural choice may need an ADR alongside a feature; an ordinary
feature or user flow belongs in Gherkin rather than a new ADR.

Accepted ADRs remain immutable historical records. Existing prose may still
mention behavior. Move acceptance coverage into Gherkin as the affected area
changes; do not rewrite accepted ADRs to remove those descriptions.

## Run the scenarios

```sh
cargo test --locked --test acceptance
cargo test --locked --test acceptance -- --name "Refuse vault commands"
cargo test --locked --workspace
```

The workspace test command includes acceptance tests, so existing pre-push and
hosted Rust checks run them. The hk test step also selects `.feature` changes.
No Node.js runner, browser, or separate Cucumber installation is needed.
Undefined or skipped steps fail the suite rather than silently passing.
Each scenario gets its own temporary directory and invokes the built binary;
shared test setup clears inherited Git-hook and CI context.

## Add or migrate a feature

1. Write a feature in domain language with concrete observable outcomes. Use a
   scenario outline for examples of the same behavior, not unrelated flows.
2. Reuse steps where they mean the same thing. Add Rust steps for new actions or
   outcomes, keeping setup and implementation detail out of the feature text.
3. Run the acceptance target. Verify a new assertion catches the intended
   failure, rather than merely checking that the scenario executed.
4. When migrating an existing end-to-end contract, remove its old test only
   after the scenario covers the same assertions. Keep unit and integration
   tests that protect distinct parser, platform, transaction, or lifecycle risks.

The initial suite migrates vault-required command failures, staged checks
outside Git, and empty staged-scope output formats from `tests/cli_workflows.rs`.
Remaining workflow tests stay in place until migrated; this is not a claim that
all CLI or editor behavior already has a Gherkin specification.

This is development-time acceptance testing for criv itself. It adds no Gherkin
parser, feature graph, or Cucumber runtime dependency to the shipped CLI and does
not change the separate Concord integration described in the repository README.
