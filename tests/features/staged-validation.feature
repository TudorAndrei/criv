Feature: Validate staged documentation changes
  A contributor can check staged changes without loading the whole vault,
  but only when a Git repository defines that staged scope.

  Scenario: Refuse a staged check outside a Git repository
    Given an initialized vault without a Git repository
    When I run criv check --changed
    Then the command fails
    And stderr contains "failed to open repository"

  Scenario Outline: Report an empty staged scope in the requested format
    Given an initialized vault without a Git repository
    And a Git repository with no staged changes
    When I check staged changes in <format> format
    Then the command succeeds
    And stdout is "<output>"

    Examples:
      | format | output           |
      | text   | criv check: ok\n |
      | json   | []\n             |
      | github |                  |
