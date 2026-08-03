---
id: ADR-0064
kind: decision
title: Typed Query Subcommands
status: accepted
date: 2026-08-03
governs:
  - src/query.rs
  - src/lib.rs
---

# Typed Query Subcommands

## Context

[[0019-export-usage-cli-spec|ADR-0019]] makes the Clap command tree the
authoritative source for parsing, runtime help, the exported Usage
specification, and downstream completions and documentation. The top-level
commands follow that model, but `criv query` still accepts an untyped query name,
a vector of positional values, and every query-related flag before dispatching
through a string match in `src/query.rs`.

That exception keeps query names outside the command tree. The names are copied
into top-level after-help text and `docs/query-reference.md`, generated
completions cannot offer them, and unknown names reach a stale runtime "MVP"
error. Because `--by`, `--kind`, and `--without-docs` are fields of the shared
query options struct, Clap accepts them for operations that ignore them.
Invalid `--by` and `--kind` values also fall through to broader default output
instead of being rejected.

## Decision

Represent every supported `criv query` operation as a variant of a Clap-derived
subcommand enum. Each variant owns its positional arguments and applicable
flags. Shared output formatting may be flattened into the variants, but
operation-specific flags are not global query options.

Represent the closed value sets for `coverage --by` and `nodes --kind` with
typed Clap value enums. Unknown operation names, missing or extra positional
arguments, irrelevant flags, and values outside those sets fail at argument
parsing with usage exit status 2. Runtime dispatch matches the typed variants;
it does not recover operation names or argument arity from strings.

Derive query help and the exported Usage specification from the same typed
command tree. Generated completion and command-reference material must
enumerate the query variants, so the duplicated top-level query-name list is
removed.

This changes only command parsing and discoverability. Every existing valid
query continues to call the same graph operation, preserve its row ordering and
text output, and serialize the same JSON array of strings. No query operation
or result schema is added.

## Consequences

Adding or removing a query operation changes the parser, help, exported Usage
specification, completions, and generated command reference from one Rust enum.
Typos and misplaced flags fail before the vault is loaded, and their diagnostics
can list the valid operations or values.

The shared `--format` option appears in each operation's command metadata, which
adds a small amount of derive-structure repetition in exchange for keeping the
public command grammar precise.

Some invocations that previously succeeded while silently ignoring an option
now fail with status 2. Valid query output remains a compatibility boundary;
parse-error wording follows Clap and should be tested by meaning rather than as
an exact full diagnostic.
