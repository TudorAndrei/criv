---
id: ADR-20261117-date-and-slug-decision-identifiers
kind: decision
title: Date and Slug Decision Identifiers
status: accepted
date: 2026-11-17
supersedes:
  - ADR-0057
  - ADR-0063
  - ADR-0163
  - ADR-0179
governs:
  - src/adr.rs
  - src/adr/create.rs
  - src/adr/gate.rs
  - src/adr/receipt.rs
  - src/adr/governs.rs
  - src/adr/source_reconcile.rs
  - src/adr/reconcile_transaction.rs
  - src/enforce.rs
  - src/identity.rs
  - src/check.rs
  - src/query.rs
  - src/git.rs
  - src/lib.rs
policy:
  patterns:
    - id: no-private-adr-child-import
      language: rust
      rule: |
        kind: scoped_identifier
        regex: '^crate::adr::(create|gate|receipt|governs|source_reconcile|reconcile_transaction)(::|$)'
      message: Callers must use the src/adr.rs interface, not a private ADR child.
    - id: confined-repository-access-only
      language: rust
      rule: |
        all:
          - any:
              - pattern: std::fs::write($$$ARGS)
              - pattern: fs::write($$$ARGS)
              - pattern: std::fs::read_to_string($$$ARGS)
              - pattern: fs::read_to_string($$$ARGS)
              - pattern: std::fs::read($$$ARGS)
              - pattern: fs::read($$$ARGS)
              - pattern: fs::symlink_metadata($$$ARGS)
              - pattern: fs::remove_file($$$ARGS)
              - pattern: File::open($$$ARGS)
              - pattern: OpenOptions::new()
          - not:
              inside:
                pattern: |
                  mod tests { $$$ }
                stopBy: end
      message: ADR and enforcement code reads and writes repository files through RepositoryFiles; ambient file access is test-only.
---

# Date and Slug Decision Identifiers

## Context

ADRs have sequential identifiers: `ADR-NNNN` in the file `NNNN-slug.md`. Two
branches that each add an ADR take the same next number. Each pull request
compares against a base that does not yet hold the other ADR, so both pass CI
and both merge. `main` then holds two decisions with one id. Nobody can repair
it: [[0012-adr-immutability-enforcement|ADR-0012]] and
[[0073-effective-adr-governance-and-source-reconciliation|ADR-0073]] make a
published ADR immutable, and a rename is a change.

[[0057-branch-local-adr-publication-and-reconciliation|ADR-0057]] and
[[0063-adr-reconciliation-owns-its-commit|ADR-0063]] answer the race with
`criv adr reconcile --base <ref>`. The command renumbers the branch-local ADRs
before merge and proves the rename with the `.criv/adr-reconcile.json` receipt.
That answer holds only while a coordinator serializes every merge against a
pinned target. A plain merge, or a merge queue that tests against a stale base,
puts the duplicate on `main`. After the merge both ADRs are published, so
renumbering is no longer allowed.
[[0163-separate-the-adr-gate-from-one-reconcile-transaction|ADR-0163]] and
[[0179-report-every-enforcement-failure-in-one-report|ADR-0179]] give that
command its own module, `src/adr/identity_reconcile.rs`, and a CI check with
the `adr-id-conflict` code.

Other tools have the same problem. Rails changed its migration names from
sequence numbers to UTC timestamps. log4brains names ADRs `YYYYMMDD-slug` and
uses the slug as the unique id
(<https://thomvaill.github.io/log4brains/adr/adr/20201016-use-the-adr-slug-as-its-unique-id/>).
Alembic and Django keep sequences and repair a fork with an added merge
migration. That repair is additive. criv can repair only by a rename, and a
rename after the merge is the change that immutability forbids.

## Decision

The existing ADRs keep their `ADR-NNNN` ids and `NNNN-slug.md` names. These
legacy forms stay valid, and no ADR is renamed.

Every new ADR has the id `ADR-YYYYMMDD-<slug>` in the file
`docs/adr/YYYYMMDD-<slug>.md`:

- `YYYYMMDD` is the UTC date on which the ADR is created. It must be a valid
  calendar date.
- `<slug>` is the slug of the ADR `title`, as
  [[0158-give-each-note-notation-one-owner|ADR-0158]] defines it: the
  lowercase alphanumeric words of the title joined with `-`. The words of a
  dated slug must use only ASCII letters and digits, so that the file name is
  the same on every platform. A title with other letters needs a different
  title, and a title with no letters or digits names no ADR.
- The id is `ADR-` followed by the file stem.

The path is the identity. Two branches clash only when they add an ADR with the
same date and the same title. Git then reports an add/add conflict at the
merge, so the clash cannot reach `main` without a person resolving it. criv
does not allocate, reserve, or renumber ids.

`src/identity.rs` owns both id forms, both filename forms, and the policy
pattern id. This extends the ownership that ADR-0158 gives it. A policy in a
dated ADR is addressed as `ADR-YYYYMMDD-slug/local-id`, in links and in State.

`criv adr new <title> [--date YYYYMMDD]` replaces
`criv query next-adr-id`. It computes the id for the title and the date, which
is today in UTC when `--date` is absent. It writes the draft
`<adr dir>/YYYYMMDD-<slug>.md` with the frontmatter `id`, `kind: decision`,
`title`, `status: draft`, and `date`, the title as the H1, and empty Context,
Decision, and Consequences sections. Then it prints the path of the draft. It
fails, and writes nothing, when an ADR with that id or that file already
exists. The private child `src/adr/create.rs` owns the command behind the
`src/adr.rs` interface, and it writes through `RepositoryFiles`, as the
policies below require.
`next-adr-id` is removed from the docs-only query set that
[[0155-remove-likec4-from-criv|ADR-0155]] retains, and no query takes its
place: `criv adr new` creates the file instead of printing an id.

`criv enforce` rejects an added ADR file with a legacy `NNNN-` name at the
commit, push, and CI stages. The violation has the code `legacy-adr-id` and the
fix "Rename it to the file that `criv adr new <title>` creates." A branch
that has an unmerged ADR with a legacy name must create it again under its
dated name before it can merge.

Delete `criv adr reconcile`, its `.criv/adr-reconcile.json` receipt,
`src/adr/identity_reconcile.rs`, and the `adr-id-conflict` code. CI runs no
identity check before enforcement. `criv adr reconcile-sources` and its
receipt do not change; ADR-0073 continues to own them, with the embedded-Git
commit that ADR-0063 introduced.

The rest of ADR-0163 and ADR-0179 stays in force without the identity command:

- `src/adr.rs` is the interface: the `adr` command options, `run`, and
  `change_violations`. Callers use only this interface, as
  [[match:ADR-20261117-date-and-slug-decision-identifiers/no-private-adr-child-import]]
  requires.
- `src/adr/gate.rs` is the gate: immutability, the portable-link migration
  exception, the CI exception for an ADR path that did not exist at the merge
  base, and the scope-move check. It opens the repository and reads the
  receipt once, and it does not depend on `check`. It skips the scope check
  only when the previous `criv.toml` is absent. When that file cannot be read
  or parsed and the change touches a decision, that is a violation.
- `src/adr/receipt.rs` owns the source receipt, its schema and path, and its
  proofs. A file proof compares a recorded hash and mode with a commit, the
  index, or the worktree. A missing file proves absence; a failed read proves
  nothing. A receipt that does not parse is a violation. Receipts hash with
  `content_hash::of_bytes`.
- `src/adr/governs.rs` owns the exact `governs:` rewrite.
  `src/adr/reconcile_transaction.rs` owns the transaction skeleton, the
  stable-target guard, the after-write validation, the snapshot, and the Git
  index snapshot. `src/adr/source_reconcile.rs` owns only its plan, its write,
  and its receipt.
- A run opens one `RepositoryFiles` value and one Git repository. ADR and
  enforcement code has no ambient file access, as
  [[match:ADR-20261117-date-and-slug-decision-identifiers/confined-repository-access-only]]
  requires.
- Enforcement reports every failing category. The JSON report holds every
  violation, every failing code in `codes` in priority order, and the first
  code as `code` with its `fix`. The order is legacy ADR id, policy, import
  policy, ADR immutability, then validation errors. The exit code follows the
  first category.
- `check::changed_scope` is the one rule for check and enforcement. A change
  that `criv check --changed` promotes to a full check also makes commit and
  push enforcement scan every governed file.
- A staged comparison on an unborn `HEAD` has no old commit. A `HEAD` that
  cannot be read for any other reason is an error.

## Consequences

Parallel branches can add ADRs without coordination. A real clash is a Git
conflict at merge time instead of a silent duplicate on `main`. A merge queue
needs no criv step.

ADR ids are longer, and the date in a new id does not record when the decision
was accepted. A title change before the merge changes the slug, so the file and
its id must change with it. After the merge the id is fixed, as for every
published ADR.

Dated names sort after the legacy names, so `docs/adr/` stays in creation
order. The ADR index lists both forms.

An in-flight branch that adds a legacy-named ADR fails enforcement until the
author creates the ADR again under its dated name and updates its links.

Scripts that call `criv query next-adr-id` or `criv adr reconcile` fail, and
a CI step that reads the `adr-id-conflict` code no longer sees it. This is a
breaking change. The ignored `.criv/adr-reconcile.json` file is no longer read
and can be deleted.
