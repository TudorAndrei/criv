---
id: ADR-0018
kind: decision
title: Offline zizmor Actions Security Check
status: accepted
date: 2026-05-15
supersedes:
  - ADR-0013
  - ADR-0014
governs:
  - hk.pkl
  - mise.toml
  - .github/workflows/release.yml
---

# Offline zizmor Actions Security Check

## Context

[[ADR-0013]] established mise-managed hk checks and already runs actionlint for
workflow syntax and GitHub Actions semantics. criv's release workflow in
[[.github/workflows/release.yml]] builds and publishes release assets, requests
attestation and contents write permissions in the publish job, and depends on
third-party GitHub Actions.

actionlint is the right baseline linter, but it does not try to audit GitHub
Actions supply-chain and workflow-security patterns. zizmor covers that
adjacent risk class, including unsafe `uses:` references, overly broad
permissions, credential handling hazards, and template-injection patterns.

## Decision

Pin zizmor through [[mise.toml]] with the `aqua:zizmorcore/zizmor` backend and
run it from [[hk.pkl]] as part of both `pre-commit` and the full `check` hook.

The hook command is:

```sh
zizmor --offline --strict-collection .
```

Use offline mode for the default local hook path. This keeps `mise run
pre-commit` and `mise run check` deterministic, avoids requiring a GitHub token
for local validation, and prevents network availability from deciding whether a
commit can pass. `--strict-collection` turns malformed collected workflow,
action, or Dependabot inputs into check failures instead of warnings.

Do not add a separate GitHub Actions workflow for zizmor yet. The repository
currently has a tag-triggered release workflow but no general pull-request CI
workflow, so duplicating local hook policy in a new hosted workflow would widen
CI behavior before the broader CI entry point has been decided.

Existing `uses:` references in [[.github/workflows/release.yml]] must be pinned
to full commit SHAs with version comments. This satisfies the default zizmor
`unpinned-uses` policy while preserving a readable upgrade trail for action
versions.

## Consequences

Workflow and action-definition changes now run both actionlint and zizmor before
commit and during `mise run check`.

Release workflow action upgrades now require resolving the new tag to its
commit SHA and updating the adjacent version comment in the same change.

Offline mode skips zizmor audits that require GitHub API access. If the project
later adds pull-request CI or SARIF/code-scanning upload, that hosted path can
run zizmor online with the repository `GITHUB_TOKEN` while keeping local hooks
offline.

The pinned toolchain now includes another release-binary dependency managed by
mise. Updating zizmor should be treated like updating actionlint: change the
mise pin and run the hook validation path before merging.
