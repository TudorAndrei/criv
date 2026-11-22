Feature: ADR folder scope
  A repository can keep general documentation in another tool.
  criv checks decision records and their source policies.

  Scenario: Check ADRs alongside general documentation
    Given an initialized vault
    And general documentation outside the ADR folder
    And a decision that links to an external document
    When I run criv check
    Then the command succeeds
    When I run criv watch --once
    Then the command succeeds
    When I run criv enforce --stage ci
    Then the command succeeds

  Scenario: Keep ADR heading checks when external documents are present
    Given an initialized vault
    And general documentation outside the ADR folder
    And a decision that links to a missing ADR heading
    When I run criv check
    Then the command fails
    And stdout contains "broken-link"

  Scenario Outline: Keep ADR heading checks for every spelling of an ADR path
    Given an initialized vault
    And a decision whose body is "See [[<target>#Missing heading]]."
    When I run criv check
    Then the command fails
    And stdout contains "broken-link"

    Examples:
      | target                     |
      | docs/adr/0002-guide.md     |
      | ./docs/adr/0002-guide.md   |

  Scenario: Resolve an external document with a dot in its name
    Given an initialized vault
    And an external document at "docs/guides/api.v2.md"
    And a decision whose body is "See [[guides/api.v2]]."
    When I run criv check
    Then the command succeeds

  Scenario: Initialize the configured ADR folder
    Given an empty directory
    And configuration that puts decisions in "decisions"
    When I run criv init --no-skills
    Then the command succeeds
    And stdout contains "created decisions/README.md"
    When I run criv check
    Then the command succeeds

  Scenario: Initialize decisions at the repository root
    Given an empty directory
    And configuration that puts decisions in "."
    When I run criv init --no-skills
    Then the command succeeds
    And stdout contains "created README.md"
