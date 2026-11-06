---
id: ADR-0174
kind: decision
title: Guard Runtime Boundaries With Tree-Wide Policies
status: accepted
date: 2026-11-06
supersedes:
  - ADR-0058
  - ADR-0137
governs:
  - Cargo.toml
  - src/*.rs
  - src/adr/*.rs
  - src/check/*.rs
  - src/discovery/*.rs
  - src/init/templates.rs
  - src/install/*.rs
  - src/repository/*.rs
  - src/source/**/*.rs
  - src/state/*.rs
  - src/watch/*.rs
policy:
  patterns:
    - id: no-runtime-git-subprocess
      language: rust
      rule: |
        all:
          - any:
              - pattern: Command::new("git")
              - pattern: std::process::Command::new("git")
          - not:
              inside:
                pattern: |
                  mod tests { $$$ }
                stopBy: end
      message: Runtime repository reads use the embedded Git boundary in src/git.rs; the Git executable is a test fixture only.
    - id: links-only-through-the-confined-helper
      language: rust
      rule: |
        all:
          - any:
              - pattern: symlink($$$ARGS)
              - pattern: std::os::unix::fs::symlink($$$ARGS)
              - pattern: $DIR.symlink_contents($$$ARGS)
              - pattern: junction::create($$$ARGS)
          - not:
              inside:
                pattern: |
                  mod tests { $$$ }
                stopBy: end
          - not:
              inside:
                kind: function_item
                has:
                  field: name
                  regex: ^create_link$
                stopBy: end
      message: Create links through RepositoryWriteScope::link_dir, whose create_link is the only caller of the platform link primitives.
---

# Guard Runtime Boundaries With Tree-Wide Policies

## Context

[[0058-embedded-git-repository-access|ADR-0058]] makes `git2` the embedded Git
backend and prohibits `Command::new("git")` in runtime code.
[[0137-link-creation-guard-follows-the-helper|ADR-0137]] allows link creation
only through `RepositoryWriteScope::link_dir`. Each decision had a policy
pattern for a few files and an integration test for the rest of `src/`.

The two tests did not guard the tree. Each read a file only up to its first
`#[cfg(test)]` marker. Most modules declare a test-only item near the top, so
the tests read almost no code. The link test searched for
`std::os::unix::fs::symlink(`, but the Unix helper calls the capability
method `symlink_contents`. ADR-0137 states that a pattern cannot tell
production code from test code. The `not: inside: mod tests` rule in ADR-0058
already does this.

## Decision

Retain the decisions of ADR-0058 and ADR-0137. `git2` is the only runtime Git
backend, `src/git.rs` is its only production boundary, and runtime code has no
Git executable fallback. `RepositoryWriteScope::link_dir` in
`src/repository/filesystem.rs` is the only way to create a link, and its
`create_link` function is the only caller of the platform link primitives.

Guard both rules with policy patterns over every production source file.
[[match:ADR-0174/no-runtime-git-subprocess]] rejects a Git executable call
outside a `mod tests` block.
[[match:ADR-0174/links-only-through-the-confined-helper]] rejects `symlink`,
`std::os::unix::fs::symlink`, `symlink_contents`, and `junction::create`
outside a `mod tests` block and outside `create_link`.

The scope names production files only. `src/init/tests.rs` and
`src/watch/refresh/tests.rs` contain only tests, so the scope leaves them out.
A new source directory must be added to this scope or to a successor decision.

Remove `tests/runtime_git_subprocess_guard.rs` and
`tests/runtime_link_creation_guard.rs`. Remove
`tests/runtime_performance_instrumentation_guard.rs`: it searches for the names
of a measurement module that
[[0072-keep-performance-observation-outside-core|ADR-0072]] removed.

## Consequences

`criv enforce` and `criv check` report a Git subprocess or a raw link call in
any production module, with the file and line. A test module can still build
Git fixtures and links.

The policy does not see a `#[cfg(test)]` item that is not named `tests`. Such
code is production code to the policy and must not call these primitives.
