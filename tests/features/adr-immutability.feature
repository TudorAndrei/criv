Feature: Accepted decisions are immutable
  A contributor who edits a committed decision must record the change in a
  new decision, wherever the vault keeps its decision directory.

  Scenario Outline: Refuse a staged edit of a committed decision
    Given a committed vault with documentation at "<docs>" and one decision
    And the decision is edited and staged
    When I run criv enforce --stage commit
    Then the command fails
    And stderr contains "[adr-immutability-violation]"

    Examples:
      | docs |
      | docs |
      | .    |
