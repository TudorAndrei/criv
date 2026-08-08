# Get started with criv

Add this to the target repository's `mise.toml`:

```toml
[tools."github:TudorAndrei/criv"]
version = "latest"
```

Then run these commands at the repository root:

```sh
mise install
criv init
criv watch --once
criv check
```

`criv init` creates the configuration, documentation and ADR directories, local
generated state, and agent skills. It leaves existing Markdown and editor files
in place.

The optional viewer is local-only. A release archive includes it next to the
CLI. Install it into one selected editor with one of these commands:

```sh
criv install-editor --editor code
criv install-editor --editor cursor
```

Use `--dry-run` to check the selected editor and bundled viewer without an
editor change.
