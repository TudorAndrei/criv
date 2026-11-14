---
id: ADR-20261115-limit-document-processing-to-the-decision-folder
kind: decision
title: Limit document processing to the decision folder
status: accepted
date: 2026-11-15
supersedes:
  - ADR-0001
  - ADR-0002
  - ADR-0190
governs:
  - src/config.rs
  - src/vault.rs
  - src/check.rs
  - src/discovery/mod.rs
  - src/watch.rs
  - src/init.rs
---

# Limit document processing to the decision folder

## Context

A repository can already use lat.md to maintain general documentation and its
links to code. Processing the same documents in criv adds a second metadata
standard and a second documentation graph.

The user selected a smaller role for criv: check the ADR folder and keep source
policy enforcement. The general documentation graph in
[[0002-docs-and-adrs-form-the-governance-graph|ADR-0002]], the document scope in
[[0001-local-cli-vault-architecture|ADR-0001]] and the watch scope in
[[0190-judge-docs-watch-events-by-the-vault-prune-rules|ADR-0190]] need to change.

## Decision

Select documents and passive assets only from the configured ADR directory.
The default directory is `docs/adr`. Keep the existing configuration:
`vault.docs` is the parent directory and `vault.adr` is relative to that parent.
All checks, queries, state publications, and document watch events use this
selection. Initialization uses the existing configuration when one is present.

Keep ADR structure, identity, history, supersession, source references, governed
scopes, and source policy enforcement. Keep the ADR index `README.md` as the
only general note in the selected folder. General documentation outside that
folder does not require criv frontmatter and does not enter the decision graph.

An ADR can link to an external Markdown document by its repository path or by
its path relative to `vault.docs`, with an optional `.md` suffix. Check that the
file exists without reading its contents. Its metadata, headings, and outgoing
links belong to the documentation tool that owns it. An indexed ADR or source
reference takes precedence; a missing ADR heading or source symbol still fails.
Use normal Markdown links when another tool has different reference rules.

Keep the local Rust CLI and local state architecture from ADR-0001. Keep the
shared discovery and watch prune rules from ADR-0190, with the ADR directory as
the document root. No lat.md dependency or adapter is required.

## Consequences

General documents no longer appear in criv queries, coverage, or state. This is
a breaking change for consumers that used criv as a general documentation graph.
Move decision records into the configured ADR directory. Keep other documents
in the documentation tool that owns them.

Accepted ADR files retain their original bytes. Existing links to external
Markdown files can remain in those records. Only their file existence is
checked; external heading changes do not fail criv.
