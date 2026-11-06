---
id: ADR-0178
kind: decision
title: Keep Configuration as Normalized Data
status: accepted
date: 2026-11-06
governs:
  - src/config.rs
  - src/enforce.rs
  - src/discovery/mod.rs
---

# Keep Configuration as Normalized Data

## Context

[[0159-derive-configuration-defaults-from-one-type|ADR-0159]] makes
`Config::default` the one source of defaults and gives `src/config.rs` one path
normalizer and the `DenyRule` import matcher.
[[0021-audit-remediation-boundaries|ADR-0021]] keeps unknown TOML keys
ignored.

`Config` exposed its fields, so discovery normalized the vault and Source paths
a second time because nothing kept a caller from setting a path that was not normalized.
Only `criv enforce` used the compiled import scope and deny globs, but the
configuration owned them. An import policy without an `id` was reported as
`import-policy` and one without a `scope` covered every file, although the
configuration reference states that each entry needs all three keys and names
the table `[[enforce.import_policies]]`, which criv did not read. The `[state]`
table alone refused unknown keys.

## Decision

`Config` keeps its fields private and is built only by `Config::parse`, which
normalizes each path once. Callers read accessors; discovery does not
normalize again. `Config` is plain data and compares by value.

`[[enforce.imports]]` entries are `ImportPolicyConfig` data with a required
`id`, `scope`, and `deny`. `src/enforce.rs` owns the compiled import policy and
`DenyRule`, and reports an invalid glob when it runs. This refines the
`DenyRule` ownership in ADR-0159.

Every table ignores unknown keys, `[state]` included, as ADR-0021 states. The
configuration reference names the `[[enforce.imports]]` table.

## Consequences

An import policy that relied on the default `id` or `scope` must state them.
`criv check` no longer reports an invalid import glob; `criv enforce` does.
Tests build configuration from TOML text, the same way users do.
