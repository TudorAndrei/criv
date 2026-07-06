# Plan 015 CI-Native Diagnostics Spike

Date: 2026-07-06

## Scope

This spike designed CI-native diagnostics for `criv check` and `criv enforce`
and added a minimal prototype for `criv check --format github`. Default text
and JSON behavior are unchanged.

The design composes with ADR-0022: hosted CI should continue to use
`mise run check` / `hk check --all` as the validation entry point unless a
future ADR deliberately extends that boundary.

## Prototype

`criv check` now accepts:

```sh
cargo run --quiet -- check --format github
```

The format emits one GitHub workflow-command line per `Diagnostic`:

```text
::error file=docs/broken.md,line=3,title=criv broken-link::wiki-link `[[missing-note]]` does not resolve
```

Warnings use `::warning`. `line=` is omitted when the diagnostic has no line.
The prototype escapes workflow-command data: messages escape `%`, carriage
returns, and newlines; command properties also escape `:` and `,`.

Scratch-vault probe:

```text
criv: check failed
::error file=docs/broken.md,title=criv invalid-kind::note frontmatter `kind` must be `doc` or `decision`
::error file=docs/broken.md,line=3,title=criv broken-link::wiki-link `[[missing-note]]` does not resolve
```

Clean-repo probe:

```text
$ cargo run --quiet -- check --format github
# no output, exit 0
```

Current JSON shape remains stable on this repo:

```json
[]
```

## Diagnostic Surface Inventory

| Surface | Examples | Path today | Line today | Routes through `Diagnostic` | Notes |
|---------|----------|------------|------------|-----------------------------|-------|
| `check` markdown format | `markdown-format` | yes | sometimes no | yes | Rumdl warnings become criv errors. |
| `check` frontmatter/schema | `invalid-frontmatter`, `missing-id`, `invalid-kind`, `adr-filename`, `decision-location` | yes | mostly yes | yes | Good annotation fit. |
| `check` links/source references | `broken-link`, `unresolved-target`, `ambiguous-source-link`, `legacy-source-target`, `source-wikilink`, `non-portable-note-link` | yes | yes when link position is known | yes | Good annotation fit; line precision is already present for important cases. |
| `check` ADR governance | `unresolved-governs`, `unknown-supersedes`, `unknown-superseded-by`, `inconsistent-supersession`, `supersession-cycle` | yes | mixed | yes | Some repo-level findings naturally lack a line. |
| `check` C4 validation | `invalid-c4-level`, `duplicate-c4-alias`, `unresolved-c4-relationship`, `invalid-c4-dot`, `missing-c4-*`, `c4-interface-drift` | yes | mixed | yes | Warnings and errors map cleanly to annotation severity. |
| `check` ADR policy definitions | `missing-policy-pattern-*`, `duplicate-policy-pattern`, `invalid-policy-pattern`, `ambiguous-policy-pattern-body` | yes | mixed | yes | Definition failures are first-class diagnostics. |
| `check` policy matches | `policy-violation` | yes | yes | yes | Already produced from structural scan rows. |
| `enforce` validation summary | validation error/warning counts | no specific file | no | partly | `enforce` calls `check::validate`, but it does not print each validation diagnostic today. |
| `enforce` policy violations | `src/foo.rs:12: ADR-0001 policy ... matched ...` | encoded in string | encoded in string | no | Should become structured before adding `--format`. |
| `enforce` import policy violations | `src/foo.rs:12: import policy ... denies ...` | encoded in string | encoded in string | no | Same structuring need as policy violations. |
| `enforce` ADR immutability | changed/deleted/renamed accepted ADRs | encoded in string | no | no | Path exists, line often does not. |
| `enforce` native tools | Oxlint/Ruff stdout/stderr passthrough | owned by external tool | owned by external tool | no | Best left as passthrough unless criv parses specific tool output later. |

The `check` diagnostic codes found in `src/check.rs` are:

```text
adr-dir-non-decision, adr-filename, ambiguous-policy-pattern-body,
ambiguous-source-link, broken-link, c4-interface-drift, decision-location,
duplicate-c4-alias, duplicate-c4-source, duplicate-doc-pattern, duplicate-id,
duplicate-pattern-id, duplicate-policy-pattern, empty-policy-pattern,
empty-target-scope, inconsistent-supersession, invalid-adr-id, invalid-c4-dot,
invalid-c4-generated, invalid-c4-level, invalid-c4-source-placement,
invalid-frontmatter, invalid-kind, invalid-policy-pattern,
legacy-source-target, markdown-format, missing-c4-description,
missing-c4-label, missing-c4-relationship-label, missing-c4-technology,
missing-id, missing-policy-pattern-body, missing-policy-pattern-definition,
missing-policy-pattern-id, missing-policy-pattern-language,
non-portable-note-link, policy-violation, source-wikilink, supersession-cycle,
unknown-superseded-by, unknown-supersedes, unresolved-c4-relationship,
unresolved-c4-target, unresolved-governs, unresolved-pattern,
unresolved-target
```

## hk / mise Nesting Evidence

GitHub documents workflow commands as `::` lines sent over stdout, including
the `::error file=...,line=...::{message}` form:

- https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands

However, workflow-command annotations emitted inside the current hk step do not
survive as raw command lines. A temporary hk config under
`/private/tmp/criv-hk-annotation-probe` ran:

```sh
printf '::warning file=README.md,line=1::criv spike probe\n'
```

through `hk check --all --step probe --no-progress`. hk printed:

```text
probe – ::warning file=README.md,line=1::criv spike probe
```

Because the line no longer starts with `::warning`, GitHub Actions should treat
it as ordinary log text, not a workflow command. The same path matters in real
CI because `.github/workflows/ci.yml` runs:

```sh
xvfb-run -a mise run check
```

and `mise run check` is:

```sh
hk check --all
```

`mise run check --help` also reports that this task does not accept arguments,
so CI cannot currently pass `--format github` through the existing task.

`tests/cli_workflows.rs` deliberately removes `CI`, `GITHUB_ACTIONS`,
`CRIV_BASE_REF`, and `GITHUB_BASE_REF` from CLI test commands. The only live
CI detection today is in `src/enforce.rs`, where `CI=true` or
`GITHUB_ACTIONS=true` makes CI enforcement require an explicit/fetchable base
ref. There is no existing annotation logic to reuse.

## Annotations vs SARIF

GitHub annotations are the cheapest first step. They require no extra action,
no new token permission, and map directly from criv's existing diagnostic
fields: severity, code, path, line, and message. They are also ephemeral job UI,
not a persistent code-scanning database, and they are sensitive to stdout
wrapping. In this repo, hk's output prefix means annotations need a direct CI
step or a file-wrapper step outside hk.

SARIF is better for persistent code-scanning UI and historical alert handling.
The mapping is also clean: `ruleId` from `Diagnostic.code`, level from severity,
and `physicalLocation` from path and line. The cost is higher: GitHub's SARIF
upload docs require `security-events: write`, plus `actions: read` for private
repositories, while this repo's CI currently uses only `contents: read`.

Sources:

- Workflow commands: https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands
- SARIF upload permissions: https://docs.github.com/en/code-security/how-tos/find-and-fix-code-vulnerabilities/integrate-with-existing-tools/upload-sarif-file
- Actions limits are explicitly subject to change: https://docs.github.com/en/actions/reference/limits

## Recommendation

Keep the `check --format github` prototype as the first implementation step,
but do not wire it into `mise run check` yet. The follow-up should add a
GitHub Actions-only wrapper step that runs `criv check --format github`
directly and preserves the existing `mise run check` gate as ADR-0022's
authoritative validation path.

The reason is the hk evidence: raw workflow commands from inside hk are
prefixed and will not be parsed. A direct CI probe step can emit annotations
without changing local hook UX. `enforce` should wait until its violation
strings are promoted into a shared diagnostic type; otherwise the project would
grow two incompatible CI diagnostic models.

## Follow-Up Plan Outline

1. Keep `Format::Github` on `check` and add a CLI workflow test that creates a
   broken scratch vault and asserts the emitted annotation line.
2. Add a CI-only step before `mise run check`:

   ```yaml
   - name: Annotate criv check diagnostics
     run: cargo run --quiet -- check --format github
   ```

   If the step should not fail before the full hk gate, run it with
   `continue-on-error: true` or capture its exit status and continue after
   emitting annotations.
3. Leave `Run repository checks` as `xvfb-run -a mise run check` so ADR-0022's
   single validation contract remains intact.
4. Refactor `enforce` policy, import-policy, and ADR-immutability violations
   into a shared diagnostic representation before adding `enforce --format`.
5. Defer SARIF until the project explicitly wants code-scanning UI and accepts
   the `security-events: write` permission expansion.

## Verification

- `cargo fmt --check` passed.
- `cargo test check::tests::github_annotation` passed.
- `cargo run --quiet -- check --format github` passed with no output on this
  repo.
- `cargo test --workspace` passed.
