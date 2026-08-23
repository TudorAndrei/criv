---
id: configuration
kind: doc
title: criv.toml configuration reference
targets:
  symbols:
    - src/config.rs
---

# criv.toml configuration reference

`criv.toml` sits at the repository root. Every key has a default, so the file
can be empty, and `criv init` writes a working one.

The CLI is the reference for commands and flags, as
[[0134-parse-the-cli-with-usage|ADR-0134]] decided. Configuration keys have no
`--help` surface, so they are described here.

## `[vault]`

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `docs` | string | `docs` | The directory that holds vault notes. |
| `adr` | string | `adr` | The decision directory, relative to `docs`. |

Both paths stay inside the repository. An absolute path or a `..` component is
refused.

```toml
[vault]
docs = "documentation"
adr = "decisions"
```

## `[source]`

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `roots` | array of strings | `["src", "lib"]` | Directories criv indexes for the source graph. |
| `exclude` | array of glob strings | `["**/target/**", "**/node_modules/**"]` | Paths the source walk skips. |

An `exclude` glob that matches a directory prunes the whole subtree, so a large
build directory costs nothing to skip.

```toml
[source]
roots = ["src", "crates"]
exclude = ["**/target/**", "**/node_modules/**", "**/generated/**"]
```

## `[index]`

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `source` | boolean | `true` | Whether to build the source graph at all. |

Set `source = false` for a documentation-only vault. Every `criv query` that
needs source data then returns nothing.

## `[state]`

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `keep` | integer | `20` | How many state snapshots to retain under `.criv/snapshots/`. |

`keep` must be one or more. See [[state-reference|the state reference]] for what
a snapshot holds.

## `[enforce]`

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `stages` | array of strings | `["commit", "push", "ci"]` | Which stages `criv enforce` will run. |

`criv enforce --stage <name>` fails when the stage is not listed. Remove a stage
to disable its gate without uninstalling the hook.

```toml
[enforce]
stages = ["commit", "ci"]
```

## `[[enforce.import_policies]]`

An import policy fails the enforcement gate when a file in `scope` imports
something in `deny`. Each entry needs all three keys.

| Key | Type | Meaning |
| --- | --- | --- |
| `id` | string | The identifier reported with the violation. |
| `scope` | array of glob strings | Files the policy applies to. |
| `deny` | array of strings | Import targets the scope must not use. A plain string matches the module or any child of it, so `tokio` also denies `tokio::net`. A glob matches the module path with `::` written as `/`. |

```toml
[[enforce.import_policies]]
id = "domain-stays-pure"
scope = ["src/domain/**"]
deny = ["reqwest", "tokio/net/*"]
```

A match reports the `import-policy-violation` code, and the repair is either to
remove the import or to widen the policy. `criv enforce --stage ci --format json`
carries the code and the repair as fields.

## Removed keys

`[patterns.*]` is refused. Persistent named patterns belong in an ADR
`policy.patterns` entry, addressed as `ADR-NNNN/local-id`. criv reports the
replacement when it finds the old table.
