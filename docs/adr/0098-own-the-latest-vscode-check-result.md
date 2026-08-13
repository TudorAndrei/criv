---
id: ADR-0098
kind: decision
title: Own The Latest VS Code Check Result
status: accepted
date: 2026-08-12
governs:
  - extensions/vscode-criv/src/diagnostics/runs.ts
  - extensions/vscode-criv/src/commands/runner.ts
  - extensions/vscode-criv/src/extension.ts
policy:
  patterns:
    - id: no-ownerless-vscode-check-call
      language: typescript
      pattern: runCheck($STORE, $DIAGNOSTICS)
      message: Every VS Code check request must use the shared check-run owner.
    - id: no-vscode-check-through-general-progress-runner
      language: typescript
      pattern: 'runCrivWithProgress($ROOT, ["check", "--format", "json"], $$$ARGS)'
      message: VS Code check processes must use the owner-controlled check runner.
    - id: vscode-check-needs-owned-publication
      language: typescript
      rule: |
        all:
          - pattern: 'async function runCheck($$$ARGS): $RETURN { $$$BODY }'
          - not:
              has:
                pattern: $OWNER.run($$$RUN_ARGS)
                stopBy: end
      message: The VS Code check entry point must publish through CheckRunOwner.
    - id: no-vscode-check-diagnostics-outside-publisher
      language: typescript
      rule: |
        all:
          - pattern: $DIAGNOSTICS.setFromJson($$$ARGS)
          - not:
              inside:
                pattern: 'async function publishCheckAttempt($$$PARAMS): $RETURN { $$$BODY }'
                stopBy: end
      message: Check diagnostics must change only in the owner-controlled publication function.
    - id: vscode-check-owner-run-needs-cancellation
      language: typescript
      rule: |
        all:
          - kind: method_definition
          - regex: '^async run'
          - inside:
              pattern: 'export class CheckRunOwner { $$$MEMBERS }'
              stopBy: end
          - not:
              has:
                pattern: this.active?.abort()
                stopBy: end
      message: A new VS Code check run must cancel the active check.
    - id: vscode-check-owner-dispose-needs-cancellation
      language: typescript
      rule: |
        all:
          - kind: method_definition
          - regex: '^dispose'
          - inside:
              pattern: 'export class CheckRunOwner { $$$MEMBERS }'
              stopBy: end
          - not:
              has:
                pattern: this.active?.abort()
                stopBy: end
      message: VS Code check-owner disposal must cancel the active check.
---

# Own The Latest VS Code Check Result

## Context

[[0037-vscode-json-diagnostics-in-hooks|ADR-0037]] defines JSON check
diagnostics for the VS Code extension. The manual check command and the
check-on-save handler started independent `criv check` processes. Each process
could clear or replace the same diagnostic collection when it completed. An
old process could therefore finish last and replace a newer result.

[[0083-own-one-loaded-state-revision-per-editor-workspace|ADR-0083]] defines a
latest-started rule for loaded State. It does not own check processes or check
diagnostics. GitHub issue #105 requires an explicit owner for this separate
lifecycle.

## Decision

Create one `CheckRunOwner` for each active VS Code extension workspace. The
manual command and the save handler use this same owner. Each request gets a
monotonic generation. Manual and save requests have equal priority. The latest
started request is authoritative.

Starting a request invalidates and aborts the active request. Process
cancellation is best effort. The process runner sends its normal termination
request and then uses its existing forced-termination limit. Generation
identity is the authority: an old process result stays stale even when the
process ignores cancellation or completes during cancellation.

Prepare the root, configuration result, process output, and parse input before
publication. Only the current generation can clear or replace diagnostics or
start a user result message. A stale generation publishes no diagnostics and
no result or failure message. A new request can invalidate a generation while
it waits for a user message; that generation must test its abort signal before
it starts another publication action.

User cancellation of the current progress notification stops its process. It
does not clear or replace diagnostics and does not show a completion message.
A current process-start failure or JSON parse failure keeps the existing
diagnostics and shows a warning. Complete JSON output replaces the full
diagnostic collection as one publication action. Truncated standard output
cannot be parsed and clears the collection with an explicit warning, as
required by the existing bounded-output contract.

Extension disposal invalidates the current generation, aborts its process, and
prevents late publication. Closing an operating-system process handle remains
the command runner's responsibility. Check ownership does not own State loads,
watch commands, queries, or CLI snapshot maintenance.

Add deterministic tests with deferred results. Tests cover manual then save,
save then manual, a late stale success, a late stale failure, current failure,
disposal, and a process signal that is already aborted. The tests must prove
that only one result reaches the publication callback.

Add strict structural policies. They require the shared owner at the check
entry point, forbid the old general-runner check path, confine diagnostic
replacement to the owned publisher, and require cancellation during new-run
acquisition and disposal. Behavioral tests remain the proof of generation
order and exact publication counts.

## Consequences

Slow checks cannot replace newer diagnostics. Manual use and check-on-save
have one clear ordering rule. Cancellation reduces unused work, while stale
result rejection keeps correctness independent of process timing.

The extension has one additional lifecycle object and a check-specific
progress adapter. Structural enforcement stops common owner bypasses. Runtime
tests still remain necessary because syntax checks cannot prove asynchronous
order.
