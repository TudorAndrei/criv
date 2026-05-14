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
actionlint. GitHub Actions security analysis is pinned here too: zizmor checks
workflow, composite action, and Dependabot definitions for risky CI/CD patterns.
Release tooling is also pinned here: Cocogitto calculates the next SemVer
version from conventional commits, and cargo-release updates Cargo workspace
versions. `HK_PKL_BACKEND=pklr` keeps hk self-contained by avoiding a separate
`pkl` CLI requirement.

Workflow YAML under `.github/workflows/` is checked with actionlint in
`pre-commit` and the full `check` hook. zizmor runs in offline mode in the same
hooks so local validation does not require a GitHub token or network access.
This follow-up hook decision is [[ADR-0018]].

Manual task entry points are:

```sh
mise run commit-msg -- .git/COMMIT_EDITMSG
mise run pre-commit
mise run pre-push
mise run check
mise run fix
mise run release-plan
mise run release-auto
```

Use `hk validate` after editing `hk.pkl`. Prefer hk built-ins when one exists;
the commit message hook uses `Builtins.check_conventional_commit` instead of a
shell wrapper around `hk util check-conventional-commit`.
