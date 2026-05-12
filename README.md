# criv

`criv` turns a repository's `docs/` folder into a validated map of documentation, code references, and architectural decisions.

This repository currently contains a dependency-light Rust MVP for the CLI and vault model. It implements:

- `criv init` for idempotent vault scaffolding
- `criv check` for note schema, ADR placement, wiki-link, target, pattern-reference, and supersession validation
- `criv query next-adr-id`
- `criv query targets <note-id>`
- `criv query cites <note-id>`
- `criv query cited-by <note-id>`
- `criv query orphan-docs`
- `criv search --files <query>`
- `criv search --grep <text>`
- `criv search --notes <text>`

The tree-sitter, ast-grep, fff-search, fastembed, watcher, enforcement, and full Obsidian rendering integrations are intentionally isolated as the next implementation layer.

## Try It

```sh
cargo run -- check
cargo run -- query next-adr-id
cargo run -- search --files main
```

## Vault Layout

```text
criv.toml
docs/
  SKILL.md
  skills/
  adr/
.criv/
```

`.criv/` is local state and is ignored by git.
