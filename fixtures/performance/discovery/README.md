# File discovery scaling fixtures

These manifests generate deterministic Source, Vault, and Markdown traversal
trees at 10,000, 100,000, and 250,000 entries. The one-million-entry Markdown
manifest is an exploratory stress case.

Generate one tree outside the repository:

```sh
cargo run --locked -p criv-perf-harness \
  --bin criv-discovery-fixtures -- \
  --manifest fixtures/performance/discovery/source-10000.toml \
  --output /path/to/new/tree
```

The generated entry count excludes the fixed repository scaffold, the Git
directory, and the generator receipt. Generated files use small deterministic
content so entry count stays separate from content-reading pressure.

These trees are synthetic scaling evidence. They do not represent an observed
user project and must not be reported as a large-project workload.

Run each Source tree with both the `source-candidates` probe and the `source`
probe. The first probe measures traversal. The second probe measures the
complete Source build.

## Focused edge cases

`criv-discovery-edge-fixtures` generates one small Git repository for each
correctness case. Its receipt contains the approved target outcome and exact
path lists. Run the release probe with `--samples 1 --allow-low-samples
--dump-paths` to record the current observation. The one-sample result is
correctness evidence, not timing evidence.

The cases are:

- `matched-parity`
- `source-shape`
- `source-link`
- `vault-shape`
- `vault-link`
- `markdown-shape`
- `markdown-invalid-pattern`
- `missing-roots`
- `non-utf8-source`

The non-UTF-8 case is not representable on all filesystems. A platform that
cannot create the name must record that limit instead of using a lossy name.
