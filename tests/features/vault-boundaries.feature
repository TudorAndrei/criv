Feature: Commands require an initialized vault
  A contributor working outside a vault needs a recovery command,
  rather than validation against an unintended directory.

  Scenario Outline: Refuse vault commands before initialization
    Given an empty directory
    When I run criv <command>
    Then the command fails
    And stderr contains "[not-a-vault]"
    And stderr contains "fix: criv init"

    Examples:
      | command            |
      | check              |
      | query next-adr-id  |
      | watch --once       |
      | enforce --stage ci |
