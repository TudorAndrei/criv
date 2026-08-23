---
id: ADR-0137
kind: decision
title: Link Creation Guard Follows the Helper
status: accepted
date: 2026-08-23
supersedes:
  - ADR-0053
governs:
  - src/init.rs
  - src/check.rs
  - src/adr.rs
  - src/watch.rs
  - tests/runtime_link_creation_guard.rs
policy:
  patterns:
    - id: links-only-through-the-confined-helper
      language: rust
      pattern: "std::os::unix::fs::symlink($$$ARGS)"
      message: Create links through RepositoryWriteScope::link_dir, which confines the path and rejects symlinked parents.
    - id: junctions-only-through-the-confined-helper
      language: rust
      pattern: "junction::create($$$ARGS)"
      message: Create Windows junctions through RepositoryWriteScope::link_dir, which confines the path and rejects symlinked parents.
---

# Link Creation Guard Follows the Helper

## Context

[[0053-claude-skills-is-a-link|ADR-0053]] decided that `criv init` links
`.claude/skills` to `.agents/skills` instead of copying, and guarded the
decision with an inline policy pattern so a raw `symlink` call could not appear
beside the confined helper.

The guard stopped guarding. It names `util::link_dir_in` as the single confined
helper, and that function does not exist anywhere in the codebase. Link creation
now happens in `RepositoryWriteScope::link_dir`, which reaches
`symlink_contents` and `junction::create` in `src/repository/filesystem.rs`.

A policy pattern is only evaluated against the files in its decision's
`governs:` list. ADR-0053 governs `src/init.rs` and `src/check.rs`, and neither
has contained a link call since the helper moved. So the pattern matched nothing
and scanned the wrong two files. A raw `std::os::unix::fs::symlink` added to
`src/repository/`, `src/install/`, or `src/state/` would have passed
`criv enforce` cleanly.

The Windows path was never guarded at all. `junction::create` has no pattern.

## Decision

Supersede [[0053-claude-skills-is-a-link|ADR-0053]]. The decision it recorded
stands unchanged: `criv init` links `.claude/skills` to `.agents/skills`, and
link creation goes through one confined helper.

Name the helper that exists. `RepositoryWriteScope::link_dir` is the only way to
create a link, and `src/repository/filesystem.rs` is the only file that calls
the platform primitives.

Guard both platforms. `junctions-only-through-the-confined-helper` covers
`junction::create` the way the Unix pattern covers `symlink`.

Guard the whole source tree with a test, because the policy cannot. An ast-grep
pattern sees text, not configuration, so it cannot tell a production call from
one inside `#[cfg(test)]`. Several modules create links legitimately in test
fixtures. `tests/runtime_link_creation_guard.rs` walks every file under `src/`,
splits each at `#[cfg(test)]`, and asserts that only
`src/repository/filesystem.rs` reaches the platform primitives.

Scope the policy to the modules the test cannot help with less: `src/init.rs`,
`src/check.rs`, `src/adr.rs`, and `src/watch.rs` are mutation-capable and use
only `symlink_metadata`, which reads rather than creates. The policy catches a
raw call there at `criv enforce` time, before the test suite runs.

## Consequences

Two guards now cover link creation, and they fail in different places. The
policy fails during `criv check` and `criv enforce` for the four scoped
modules. The source guard fails during `cargo test` for the whole tree,
including modules whose test fixtures make a policy pattern useless.

The source guard also fails when the helper moves, naming the constant to
update. That is the failure ADR-0053 did not have: its guard went quiet instead.

A file whose entire contents are tests, such as `src/init/tests.rs`, is skipped
by the source guard, because there is no `#[cfg(test)]` marker inside it to
split on. Those files are the remaining blind spot.
