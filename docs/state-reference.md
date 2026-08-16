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

Each `source-index` entry contains `path` and `mime`. New State output does not
contain `frecency`. A current reader accepts an older `criv.state.v1` document
that has the extra field and ignores it. The schema name does not change
because stored JSON readers already accept unknown fields. Rust users of
`criv-state-wire::SourceIndexEntry` must remove direct use of the deleted
field when they update.

Snapshot retention is automatic. The CLI does not expose snapshot list or
prune commands. Corrupt recognized snapshots fail closed and are not deleted
automatically.

`criv query diff <a> <b>` resolves `latest` and retained hashes from the local
store. Other values remain embedded Git-ref lookups of `.criv/state.json`.
Pruning local snapshots does not inspect or alter Git refs.

The lifecycle and recovery contract are defined by
[[0094-automatic-recoverable-state-publication|ADR-0094]].
The Source entry change is defined by
[[0112-direct-ignore-file-discovery|ADR-0112]].
