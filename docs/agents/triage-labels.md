---
id: agents-triage-labels
kind: doc
title: Triage Labels For Agent Skills
---

# Triage Labels

The skills speak in terms of five canonical triage roles. This file maps those
roles to the label strings used in this repo's GitHub Issues.

| Label in mattpocock/skills | Label in our tracker | Meaning                                  |
| -------------------------- | -------------------- | ---------------------------------------- |
| `needs-triage`             | `needs-triage`       | Maintainer needs to evaluate this issue  |
| `needs-info`               | `needs-info`         | Waiting on reporter for more information |
| `ready-for-agent`          | `ready-for-agent`    | Fully specified, ready for an AFK agent  |
| `ready-for-human`          | `ready-for-human`    | Requires human implementation            |
| `wontfix`                  | `wontfix`            | Will not be actioned                     |

When a skill mentions a role (e.g. "apply the AFK-ready triage label"), use the
corresponding label string from this table.

Only `wontfix` exists in the repository today; the other four are the GitHub
defaults' gap and must be created before first use:

```sh
gh label create needs-triage --description "Maintainer needs to evaluate this issue"
gh label create needs-info --description "Waiting on reporter for more information"
gh label create ready-for-agent --description "Fully specified, ready for an AFK agent"
gh label create ready-for-human --description "Requires human implementation"
```

## Terminal states are decisions

This repository records decisions as ADRs under `docs/adr/`, and the two
terminal triage outcomes are decisions.

**Closing as done.** When the work settled a question about how criv behaves —
what a command does, what it refuses to do, what a generated artifact looks like
— write an ADR before closing. Pure implementation of an already-decided
behavior does not need one; changing or establishing a behavior does.

**Closing as `wontfix`.** A `wontfix` is a decision not to do something, and the
reasoning is worth more than the label. Write an ADR recording what was proposed
and why it was rejected, then close with a link to it.

Both cases follow the normal ADR workflow:

- `criv query next-adr-id` for the number; never guess it.
- Required frontmatter is `id`, `kind: decision`, `title`, `status`, `date`.
- Use `governs:` for the path globs the decision controls.
- Accepted ADRs are immutable under ADR-0012. To change a past decision, write a
  new ADR with `supersedes:` pointing at the old one — never edit it.
- Run `criv check` before finishing.

An issue closed as done or `wontfix` should link the ADR that settled it, so the
reasoning survives the issue being closed.
