---
id: ADR-0176
kind: decision
title: Guard Runtime Boundaries Over the Whole Source Tree
status: accepted
date: 2026-11-06
supersedes:
  - ADR-0174
governs:
  - Cargo.toml
  - src/**/*.rs
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
          - not:
              inside:
                kind: function_item
                follows:
                  pattern: "#[test]"
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
                follows:
                  pattern: "#[test]"
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

# Guard Runtime Boundaries Over the Whole Source Tree

## Context

[[0174-guard-runtime-boundaries-with-tree-wide-policies|ADR-0174]] guards the
embedded Git boundary and the confined link helper with two policy patterns.
Its scope lists the `src` subdirectories one by one, so that it can leave out
`src/init/tests.rs` and `src/watch/refresh/tests.rs`. Those files hold only
tests, but their test functions are not inside a `mod tests` block. A new
source directory is outside the scope until a later decision adds it, so a
violation in that directory passes without a report.

## Decision

Retain the rules of ADR-0174: `src/git.rs` is the only production Git
boundary, and `create_link` in `src/repository/filesystem.rs` is the only
caller of the platform link primitives.

Govern every Rust file under `src`. Each policy also exempts the body of a
function whose attribute directly before it is `#[test]`, in addition to a
`mod tests` block. A test file can then build Git fixtures and links, and a new
directory is in the scope from its first file.
[[match:ADR-0176/no-runtime-git-subprocess]] and
[[match:ADR-0176/links-only-through-the-confined-helper]] replace the ADR-0174
patterns.

## Consequences

`criv enforce` and `criv check` report a Git subprocess or a raw link call in
any production module, including a new directory, with the file and line.

A helper function in a test-only file that is not itself a `#[test]` function
is production code to the policy and must not call these primitives. Such a
helper must live in a `mod tests` block or take the primitive from a test
function.
