Feature: Name new decisions by date and title
  A new decision takes the id ADR-YYYYMMDD-slug from its date and title, and
  its file is that id without "ADR-". Decisions written on parallel branches
  therefore do not collide. Published decisions keep their legacy ADR-NNNN ids.

  Scenario: Create the draft of a new decision
    Given an initialized vault
    When I create the decision "Use: a merge queue" on 20261117
    Then the command succeeds
    And stdout is the line "docs/adr/20261117-use-a-merge-queue.md"
    And the file "docs/adr/20261117-use-a-merge-queue.md" is:
      """
      ---
      id: ADR-20261117-use-a-merge-queue
      kind: decision
      title: 'Use: a merge queue'
      status: draft
      date: 2026-11-17
      ---

      # Use: a merge queue

      ## Context

      ## Decision

      ## Consequences
      """

  Scenario: The created draft passes the check
    Given an initialized vault
    And a Rust source
    When I create the decision "Use a merge queue" on 20261117
    And I run criv check
    Then the command succeeds

  Scenario Outline: Refuse an id that the vault already holds
    Given an initialized vault
    And a decision "Choose Storage" in the file "<file>" with id "ADR-20261117-choose-storage"
    When I create the decision "Choose Storage" on 20261117
    Then the command fails
    And stderr contains "`ADR-20261117-choose-storage` already exists in the vault at `docs/adr/<file>.md`"
    And the ADR files are "<file>.md"

    Examples:
      | file                    |
      | 20261117-choose-storage |
      | 20261118-choose-storage |

  Scenario: Refuse a title without a slug
    Given an initialized vault
    When I create the decision "!!!" on 20261117
    Then the command fails
    And stderr contains "slug"
    And the ADR files are ""

  Scenario Outline: Check the id and file name of a decision
    Given an initialized vault
    And a decision "<title>" in the file "<file>" with id "<id>"
    When I run criv check
    Then the command <result>
    And stdout contains "<code>"

    Examples:
      | title           | file                     | id                           | result   | code           |
      | Choose Storage  | 20261117-choose-storage  | ADR-20261117-choose-storage  | succeeds |                |
      | Choose Storage  | 0007-choose-storage      | ADR-0007                     | succeeds |                |
      | Choose Storage  | 20261131-choose-storage  | ADR-20261131-choose-storage  | fails    | invalid-adr-id |
      | Choose Database | 20261117-choose-storage  | ADR-20261117-choose-storage  | fails    | invalid-adr-id |
      | Choose Storage  | 20261118-choose-storage  | ADR-20261117-choose-storage  | fails    | adr-filename   |
      | Choose Storage  | 0007-choose-storage      | ADR-20261117-choose-storage  | fails    | adr-filename   |
