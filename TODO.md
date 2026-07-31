# TODO: Skill staleness nudge and `--force-skills` refresh

## Completed

- [x] Stamp generated skills with stable, content-sensitive BLAKE3 markers.
- [x] Keep shipped assets marker-free and inject valid, idempotent frontmatter at write time.
- [x] Add confined `criv init --force-skills` refreshes, reporting each refreshed file.
- [x] Preserve create-only initialization, `--no-skills`, and symlink confinement.
- [x] Report stale or legacy skills only in text `criv check` output without changing diagnostics or exit status.
- [x] Preserve JSON, GitHub, and filtered diagnostic output.
- [x] Add regression coverage for current, stale, missing, markerless, and malformed skill files.
- [x] Supersede ADR-0010 with ADR-0051 and document the criv-owned artifact rule.
- [x] Update the 0.7.0 query, C4, and drift-checking skill guidance.
- [x] Re-sync `.agents/skills` and `.claude/skills`, add `.claude/skills` as a source root, and test parity with shipped templates.
- [x] Exercise `next-adr-id`, C4 query commands, `check --filter`, and the refresh flow.

## Verification

- [x] `cargo test --workspace` passed before the final full gate; all new Rust and CLI tests pass.
- [x] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [x] `cargo run --quiet -- check` and `cargo run --quiet -- enforce --stage ci` pass.
- [x] The full `mise run check` gate was run from a physically short temporary worktree to avoid the current worktree's macOS VS Code socket-path limitation.
- [x] Marked skill frontmatter is parsed as valid YAML by the local interoperability check.

## Review

- [x] Phase commits are clean and use the planned conventional messages.
- [x] ADR-0051 explicitly documents that refreshes replace criv-owned generated skills.
- [x] PLAN.md records the resolved count-only and missing-skill decisions.
