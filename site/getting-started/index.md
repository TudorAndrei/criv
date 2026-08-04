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
generated state, agent skills, and editor companion files. It leaves existing
Markdown files in place.
