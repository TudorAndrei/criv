---
id: ADR-0159
kind: decision
title: Derive Configuration Defaults from One Type
status: accepted
date: 2026-11-05
governs:
  - src/config.rs
  - src/init/templates.rs
  - src/init/fixtures/criv.toml
---

# Derive Configuration Defaults from One Type

## Context

[[0062-canonical-generated-criv-configuration|ADR-0062]] ships the default
`criv.toml` as a checked-in template. It kept a test-only `DefaultConfig` type
as the semantic oracle for that template. `Config::default` in
`src/config.rs` held the same values a third time, and nothing compared it with
the other two.

The rest of the configuration had the same kind of copies:

- `criv enforce` and the ADR gate each had an enum for the commit, push, and CI
  stages, and `Config` held the configured stages as strings.
- An import policy kept its deny patterns and their compiled globs in two
  vectors that had to stay aligned by index.
- Source roots and vault paths had two normalizers with different rules for a
  `..` component.
- Six modules spelled `criv.toml`.
- With `vault.docs = "."`, the ADR prefix became `./adr/`. Git paths never
  start with `./`, so the ADR immutability gate matched no file.

## Decision

`Config::default` is the only source of default configuration values. A test
parses the template and requires the result to equal `Config::default`, and
requires the template to state each key rather than rely on a default. The
`DefaultConfig` type is gone. The template stays a checked-in file, as ADR-0062
decides, for its exact bytes.

`src/config.rs` also owns:

- `Stage`, the typed `commit`, `push`, and `ci` stages. The CLI, the stage
  list in `criv.toml`, and the ADR gate use it. An unknown stage name in
  `criv.toml` is a configuration error.
- `DenyRule`, which holds a deny pattern with its optional glob and decides
  whether an import matches.
- one normalizer for configured paths. A path stays inside the repository: an
  absolute path or a `..` component is refused.
- `CONFIG_FILE`, the one spelling of `criv.toml`, and `open_vault`, which
  refuses a directory without it.
- `adr_prefix`, which leaves out a `.` directory, so `vault.docs = "."` gives
  `adr/`.

This refines ADR-0062. Its decision about the canonical template and the
formatter configuration does not change.

## Consequences

A new configuration key changes `Config` and the template, and the test fails
until both agree.

A vault whose documentation is the repository root is protected by the ADR
immutability gate. Before this decision, the gate did not see its decisions.

A `source.roots` entry with a `..` component, such as `src/../lib`, is now
refused. Write the normalized path instead.
