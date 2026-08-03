---
id: state-reference
kind: doc
title: Local State And Snapshot Reference
---

# Local State And Snapshot Reference

`criv watch --once` and live watch publication write the current graph to
`.criv/state.json`. They also keep content-addressed local snapshots for
`criv query diff`. The snapshot store is ignored local data; source control
remains the durable history boundary.

Configure the maximum number of distinct local snapshots in `criv.toml`:

```toml
[state]
keep = 20
```

`keep` must be a positive integer and defaults to `20`. The bound is applied
after every successful state publication. Publishing identical content again
does not consume another slot, but makes that hash the newest publication. The
snapshot named by `.criv/latest` is always protected.

Inspect retained snapshots newest first:

```sh
criv state list
criv state list --format json
```

Preview or apply pruning with the configured bound:

```sh
criv state prune --dry-run
criv state prune
criv state prune --keep 5
criv state prune --keep 5 --format json
```

`--keep` is a one-command positive override; it does not rewrite `criv.toml`.
`--dry-run` selects and reports the same snapshots without changing local
files. Corrupt recognized snapshots fail closed and are not deleted
automatically.

`criv query diff <a> <b>` resolves `latest` and retained hashes from the local
store. Other values remain embedded Git-ref lookups of `.criv/state.json`.
Pruning local snapshots does not inspect or alter Git refs.

The lifecycle and compatibility boundary are defined by
[[0068-bounded-local-snapshot-lifecycle|ADR-0068]].
