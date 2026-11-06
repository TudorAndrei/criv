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
    And stdout contains "ADR-0999 policy"
    And stdout contains "src/lib.rs:2"

  Scenario Outline: Report the CI identity check as one JSON report
    Given a committed vault
    And a target branch that adds decision 1 "Target Choice"
    And a topic branch that adds <topic>
    When I enforce the CI stage in JSON against the target branch
    Then the command <result>
    And stdout is one JSON report with code "<code>"

    Examples:
      | topic                       | result   | code            |
      | a note                      | succeeds |                 |
      | decision 1 "Topic Choice"   | fails    | adr-id-conflict |
