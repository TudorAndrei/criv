---
id: ADR-0053
kind: decision
title: Claude Skills Is A Link Not A Copy
status: accepted
date: 2026-07-31
supersedes:
  - ADR-0052
governs:
  - src/init.rs
  - src/check.rs
policy:
  patterns:
    - id: links-only-through-the-confined-helper
      language: rust
      pattern: "std::os::unix::fs::symlink($$$ARGS)"
      message: Create links through util::link_dir_in, which confines the path and rejects symlinked parents.
---

# Claude Skills Is A Link Not A Copy

## Context

[[0010-criv-init-installs-agent-runtime-skills|ADR-0010]] had `criv init` write
the same six skill files into two trees, `.agents/skills` and `.claude/skills`,
because the two agent runtimes look in different places.

Two trees of identical files drift. This repository proved it: four of six
`.claude/skills` files fell behind `assets/skills`, and the stale copies
instructed agents to write link forms and policy layouts that the current
`criv check` rejects. Nothing detected it, because criv could not see its own
third copy.

[[0051-refresh-generated-agent-skills|ADR-0051]] and
[[0052-harden-generated-skill-refresh|ADR-0052]] made that drift *detectable* and
*fixable* — a content hash per installed file, a nudge from `criv check`, and
`criv init --force-skills` to refresh. That is a real improvement, but it treats
a symptom. One tree cannot drift from itself.

The `skills` CLI that installs agent skills across runtimes already solves this
by symlinking rather than copying, and explicitly supports the exact layout
proposed here — its `createSymlink` resolves parent symlinks so that
`~/.claude/skills -> ~/.agents/skills` is recognised as the same physical
directory rather than a conflict.

The obstacle is criv's own rule.
[[0044-vault-write-confinement|ADR-0044]] rejects writes through any symlinked
path component, and correctly so: a symlink's intent cannot be inferred, so the
safe policy is refusal. A vault that symlinked `.claude/skills` by hand today
gets `refusing to write through symlinked vault path component`.

## Decision

`.claude/skills` is a link to `.agents/skills`, created by criv. It is never a
second copy of the files.

criv writes skill files to exactly one location, `.agents/skills`. It never
writes *through* the link, so ADR-0044's confinement is unweakened: every parent
component is still rejected if symlinked, and only the final component — which
criv itself creates and owns — may be a link.

Link creation goes through `util::link_dir_in`, the single confined helper. The
inline policy on this decision enforces that: raw `std::os::unix::fs::symlink`
calls outside that helper are a policy violation, because they would bypass the
path validation and parent-component rejection that make creating a link safe at
all.

Collapsing an existing `.claude/skills` directory deletes the copies it holds, so
it requires `criv init --force-skills`. A plain `criv init` reports the directory
and leaves it alone. This follows the rule established by ADR-0051: criv nudges,
the user acts, because these files are tracked by git and a silent rewrite would
produce surprise working-tree changes.

`criv check` reports a real `.claude/skills` directory as out of date, alongside
stale content hashes, so a pre-link vault is told what to do rather than left to
discover it.

On Windows the link is an NTFS junction, created through the `junction` crate.
A junction needs no Developer Mode and no elevation, unlike the directory
symlink in `std::os::windows`, so an ordinary Windows checkout gets the same
single-tree layout as everywhere else. The crate is declared under
`[target.'cfg(windows)'.dependencies]`, so no other platform resolves it, and its
only runtime dependencies are `scopeguard` and Microsoft's own `windows-sys`.
This mirrors the `skills` CLI, which picks a junction on `win32` for the same
reason.

Junctions require an absolute target, POSIX symlinks take a relative one, so the
link target differs by platform: relative elsewhere so the vault stays movable,
absolute on Windows because the API demands it.

Where the platform still cannot create a directory link — a non-NTFS volume, or
any other failure — criv writes the copies as before and says so. Such a vault
keeps the staleness nudge from ADR-0051 as its drift defence.

`.claude/skills` is removed from `criv.toml`'s source roots. Indexing both ends
of a link would count every skill file twice.

## Consequences

The drift this repository suffered becomes structurally impossible rather than
merely detectable. `assets/skills` is the shipped template, `.agents/skills` is
the single installed tree, and `.claude/skills` resolves to it.

Filesystems without link support keep two trees and therefore keep the drift
risk. They also keep the ADR-0051 nudge, which is what makes that risk
survivable.

criv gains its first platform-specific dependency. ADR-0003 curates the
dependency set deliberately, and this is admitted on the grounds that the
alternative is a Windows-only degradation of a core layout decision, and that
the crate is narrow, widely used, and target-gated.

Git stores the link, so a clone reproduces the layout. A Windows checkout without
`core.symlinks` materialises a text file containing the target path; running
`criv init --force-skills` there replaces it with a junction.

The Windows path is exercised by a `windows-2025` job in CI, because it cannot be
compiled or run on the maintainer's machine.

Superseding ADR-0052 retires nothing it decided. The content hash, the advisory
nudge, the text-format-only rule, and the narrow `--force-skills` scope all
stand. What changes is that they now guard one tree instead of two.
