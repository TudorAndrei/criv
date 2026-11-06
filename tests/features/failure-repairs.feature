Feature: Name the repair for a failed run
  An agent that sees a failed check or enforcement run needs the next step,
  not only the failure code.

  Scenario: Name the repair when a check fails
    Given a committed vault
    And a committed note "guide" that links to "missing"
    When I run criv check
    Then the command fails
    And stderr contains "[check-failed]"
    And stderr contains "fix: Repair the diagnostics above, then run `criv check`."

  Scenario: Name the repair in the JSON report when CI enforcement fails
    Given a committed vault
    And a committed note "guide" that links to "missing"
    When I run criv enforce --stage ci --format json
    Then the command fails
    And stdout is one JSON report with code "enforcement-failed"
    And the JSON report fix is "Run `criv check` for the diagnostics, then repair them."
