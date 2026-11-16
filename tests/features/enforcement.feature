Feature: Enforce the decisions of a Git transaction
  Enforcement scans what a transaction can change. A transaction that adds or
  changes a decision can change what every governed file must satisfy, so it
  is scanned as a whole.

  Scenario: Enforce a staged decision's policy on unchanged governed sources
    Given a committed vault
    And a committed Rust source that calls println
    And a staged decision that forbids println in Rust sources
    When I run criv enforce --stage commit
    Then the command fails
    And stdout contains "ADR-20261117-no-println policy"
    And stdout contains "src/lib.rs:2"
    And stdout does not contain "legacy number"

  Scenario Outline: Report new decision names in CI as one JSON report
    New decisions take dated ids, so branches cannot collide. Only a
    published decision keeps its legacy number.

    Given a committed vault
    And a topic branch that adds <topic>
    When I enforce the CI stage in JSON against the main branch named by <variable>
    Then the command <result>
    And stdout is one JSON report with code "<code>"

    Examples:
      | topic                            | variable        | result   | code          |
      | a note                           | CRIV_BASE_REF   | succeeds |               |
      | dated decision "Topic Choice"    | CRIV_BASE_REF   | succeeds |               |
      | legacy decision 1 "Topic Choice" | CRIV_BASE_REF   | fails    | legacy-adr-id |
      | legacy decision 1 "Topic Choice" | GITHUB_BASE_REF | fails    | legacy-adr-id |

  Scenario: Refuse a staged decision with a legacy number
    Given a committed vault
    And a staged legacy decision 7 "Topic Choice"
    When I run criv enforce --stage commit --format json
    Then the command fails
    And stdout is one JSON report with code "legacy-adr-id"
    And stdout contains "new ADR `docs/adr/0007-topic-choice.md` uses a legacy number; new ADRs are named YYYYMMDD-slug.md"
    And the JSON report fix is "Rename it to the file that `criv adr new <title>` creates."
