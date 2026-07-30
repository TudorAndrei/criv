---
id: agents-issue-tracker
kind: doc
title: Issue Tracker For Agent Skills
---

# Issue tracker: GitHub Issues

Issues and specs (you may know a spec as a PRD) for this repo live in GitHub
Issues on `TudorAndrei/criv`, reached through the `gh` CLI.

## When a skill says "publish to the issue tracker"

```sh
gh issue create --title "<title>" --body "<body>" --label "<label>"
```

## When a skill says "fetch the relevant ticket"

```sh
gh issue view <number>
gh issue view <number> --comments
gh issue list --label ready-for-agent
```

The user will normally pass the issue number or URL directly.

## When a skill says "comment on the ticket"

```sh
gh issue comment <number> --body "<body>"
```

## PRs as a request surface

**Off.** Skills read and write issues only; incoming pull requests are not part
of the triage queue. Flip this section on if that changes.

## There is no second tracker

GitHub Issues is the only issue tracker for this repository. An `ISSUES.md` file
at the repo root previously held a hand-curated audit index; it was migrated to
GitHub Issues on 2026-07-30 and deleted. Do not recreate it, and do not add a
parallel findings file — a second list drifts from the first.

Issues migrated from it carry a footer naming their original `ISSUES.md` number,
because ADRs and commit messages written before the migration refer to those
numbers.

## Issue quality

The migrated issues set the bar for a substantial finding: a one-line statement
of the defect, then `Evidence` with `file:line` citations, `Impact`, a
`Fix sketch`, and `Verification`. Match that shape when filing something
non-trivial. A finding without evidence someone else can check is not ready to
file.

## Closing an issue

Terminal outcomes are decisions, and decisions in this repository live in
`docs/adr/`. See `docs/agents/triage-labels.md` for the rule: closing an issue as
done or as `wontfix` requires an ADR when it settles a question about how criv
behaves.
