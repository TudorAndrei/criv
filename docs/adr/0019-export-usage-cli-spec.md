---
id: ADR-0019
kind: decision
title: Export Usage CLI Spec and Help
status: accepted
date: 2026-06-09
governs:
  - Cargo.toml
  - src/lib.rs
  - README.md
---

# Export Usage CLI Spec

## Context

[[0001-local-cli-vault-architecture|ADR-0001]] makes the Rust CLI the public interface for humans, agents, hooks,
and downstream tooling. [[0003-adopt-proven-foundation-crates|ADR-0003]] already chose Clap as the command parser in
`src/lib.rs`, so criv has one authoritative command tree for help text,
subcommands, flags, and parser behavior.

Users still need shell completions, generated command reference material, and
machine-readable CLI metadata. Maintaining a separate CLI spec by hand would
create another drift point beside the Clap definitions.

The `usage` project provides a KDL CLI specification that can render CLI help
and generate shell completions, Markdown documentation, manpages, SDKs, and
parser integrations. Its Rust `usage-lib` crate can derive that spec from an
existing Clap `Command`.

## Decision

Add `usage-lib` as a runtime dependency in `Cargo.toml`, with the resolved
dependency graph recorded in `Cargo.lock`. Derive a Usage spec from the existing
Clap command tree in `src/lib.rs`.

Use that derived spec for the hidden top-level `criv --usage` KDL export and for
runtime help rendering. `criv --help`, `criv help`, `criv help <command>`, and
`criv <command> --help` should render from Usage rather than from Clap's default
help printer.

Keep Clap as the argument parser and command dispatcher. Usage owns the
presentation of help and generated metadata, but command execution should remain
typed through the existing Clap-derived structs.

Do not vendor or depend on the standalone `usage-cli` binary at runtime. Users
who want generated assets can install `usage-cli` separately and pipe criv's
spec into it:

```sh
criv --usage | usage generate completion --file - zsh criv
criv --usage | usage generate markdown --file - --out-file docs/cli.md
criv --usage | usage generate manpage --file - --out-file criv.1
```

Document the spec export in `README.md` without showing it in normal help
output.

## Consequences

Shell completion, Markdown, and manpage generation can share one source of
truth with the Clap parser.

Adding `usage-lib` brings in the Usage spec model and KDL rendering stack. That
dependency cost is acceptable because the emitted spec is directly tied to the
CLI public surface, renders the interactive help path, and avoids
hand-maintained generated documentation.

Changes to CLI command structure should keep the generated spec test in
`src/lib.rs` passing, and any generated command-reference docs should be
treated as build artifacts unless a later ADR decides to check them in.
