---
id: query-reference
kind: doc
title: criv query reference
targets:
  symbols:
    - src/query.rs
---

# criv query reference

`criv query` asks the loaded vault graph focused questions and prints one row
per result. Add `--format json` to any query for a JSON array of rows:

```sh
criv query nodes --kind decision --format json
```

## Subcommands

| Token | Positional argument | Flags | Description | Example |
| --- | --- | --- | --- | --- |
| `next-adr-id` | - | `--format` | Prints the next ADR id after the highest existing `ADR-####` id. | `criv query next-adr-id` |
| `callers` | `<symbol>` | `--format` | Lists source symbols that call the requested symbol. | `criv query callers src/query.rs#fn:load_snapshot` |
| `callees` | `<symbol>` | `--format` | Lists source symbols called by the requested symbol. | `criv query callees src/query.rs#fn:diff` |
| `attack-surface` | - | `--format` | Lists exported or public source symbols in the source graph. | `criv query attack-surface` |
| `targets` | `<note-id>` | `--format` | Lists source and pattern targets declared or linked by a note. | `criv query targets tooling` |
| `cites` | `<note-id>` | `--format` | Lists notes, sources, and patterns cited by a note. | `criv query cites ADR-0034` |
| `cited-by` | `<note-id>` | `--format` | Lists notes that cite the requested note. | `criv query cited-by ADR-0034` |
| `orphan-docs` | - | `--format` | Lists `kind: doc` notes with no incoming or outgoing note citations. | `criv query orphan-docs` |
| `references` | `<symbol>` | `--format` | Lists notes that reference a source path or symbol. | `criv query references mise.toml` |
| `governs` | `<ADR-ID>` | `--format` | Lists source files governed by a decision's effective scopes. | `criv query governs ADR-0005` |
| `governing` | `<symbol>` | `--format` | Lists decisions whose effective scopes govern a source path or symbol. | `criv query governing src/query.rs` |
| `coverage` | - | `--by module\|adr`, `--format` | Summarizes source governance coverage globally, by module, or by ADR. | `criv query coverage --by module` |
| `nodes` | - | `--kind code\|doc\|decision`, `--without-docs`, `--format` | Lists source, note, or decision nodes; `--without-docs` filters documented code symbols. | `criv query nodes --kind code --without-docs` |
| `c4-elements` | `<note-id>` | `--format` | Lists parsed Mermaid C4 elements from a note. | `criv query c4-elements ADR-0026` |
| `c4-relationships` | `<note-id>` | `--format` | Lists parsed Mermaid C4 relationships from a note. | `criv query c4-relationships ADR-0026` |
| `c4-code` | `<path-glob>` | `--format` | Emits a focused Mermaid class diagram from source graph symbols and calls. | `criv query c4-code src/query.rs` |
| `diff` | `<ref-a> <ref-b>` | `--format` | Compares two state snapshots or git refs and lists added/removed graph nodes and edges. | `criv query diff latest latest` |

`diff` resolves `latest` through `.criv/latest`, hex-like values through
`.criv/snapshots/<hash>.json`, and any other value through
an embedded lookup of `.criv/state.json` in the requested repository ref. It
does not invoke the `git` executable.
