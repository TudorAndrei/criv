---
id: tooling
kind: doc
title: Tooling and Git Hooks
targets:
  symbols:
    - mise.toml
    - hk.pkl
---

# Tooling and Git Hooks

The repository uses [[mise.toml]] to install the project hook runner and
[[hk.pkl]] to define the hook behavior. The decision record is [[ADR-0013]].

Run the initial setup with:

```sh
mise install
```

The mise postinstall hook runs `hk install --mise`, so Git hook execution goes
through `mise x` and uses the pinned tool versions, including hk and
actionlint. `HK_PKL_BACKEND=pklr` keeps hk self-contained by avoiding a separate
`pkl` CLI requirement.

Workflow YAML under `.github/workflows/` is checked with actionlint in
`pre-commit` and the full `check` hook.

Manual task entry points are:

```sh
mise run commit-msg -- .git/COMMIT_EDITMSG
mise run pre-commit
mise run pre-push
mise run check
mise run fix
```

Use `hk validate` after editing `hk.pkl`. Prefer hk built-ins when one exists;
the commit message hook uses `Builtins.check_conventional_commit` instead of a
shell wrapper around `hk util check-conventional-commit`.
