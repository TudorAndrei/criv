Feature: Validate staged documentation changes
  A contributor can check staged changes without loading the whole vault,
  but only when a Git repository defines that staged scope.

  Scenario: Refuse a staged check outside a Git repository
    Given an initialized vault
    And no Git repository
    When I run criv check --changed
    Then the command fails
    And stderr contains "failed to open repository"

  Scenario Outline: Report an empty staged scope in the requested format
    Given an initialized vault
    And a Git repository with no staged changes
    When I check staged changes in <format> format
    Then the command succeeds
    And stdout is "<output>"

    Examples:
      | format  | output           |
      | default | criv check: ok\n |
      | text    | criv check: ok\n |
      | json    | []\n             |
      | github  |                  |

  Scenario: Report problems in a staged note but not in committed notes
    Given a committed vault
    And a committed note "unchanged-broken" that links to "missing-global"
    And a staged note "guide" that links to "missing-added"
    When I run criv check --changed --filter broken-link
    Then the command fails
    And stdout contains "docs/guide.md"
    And stdout does not contain "docs/unchanged-broken.md"

  Scenario: Report a problem in a staged edit of a committed note
    Given a committed vault
    And a committed note "unchanged-broken" that links to "missing-global"
    And a committed note "guide" without links
    And a staged note "guide" that links to "missing-modified"
    When I run criv check --changed --format json
    Then the command fails
    And stdout contains "missing-modified"
    And stdout does not contain "missing-global"

  Scenario Outline: Validate the whole vault after a staged <change>
    Given a committed vault
    And a committed note "unchanged-broken" that links to "missing-global"
    And a committed note "change-me" without links
    And the note "change-me" is <change> and staged
    When I run criv check --changed --filter broken-link
    Then the command fails
    And stdout contains "docs/unchanged-broken.md"

    Examples:
      | change   |
      | renamed  |
      | deleted  |

  Scenario: Check decision policies on staged sources
    Given a committed vault
    And a committed decision that forbids println in Rust sources
    And a staged Rust source that calls println
    When I run criv check --changed --filter policy-violation
    Then the command fails
    And stdout contains "ADR-0999 policy"
    And stdout contains "src/lib.rs:2"

  Scenario: Refuse to fix files during a staged check
    Given a Git repository with no staged changes
    When I run criv check --changed --fix
    Then the command fails with exit code 2
    And stderr contains "--fix"
