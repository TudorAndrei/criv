---
id: ADR-0192
kind: decision
title: Retain explicit source lookup results after the editor-state move
status: accepted
date: 2026-11-06
supersedes:
  - ADR-0097
governs:
  - crates/criv-wasm/src/lib.rs
  - crates/criv-wasm/src/source.rs
  - packages/criv-editor-state/src/sourceTarget.ts
  - packages/criv-editor-state/src/stateModule.ts
  - extensions/vscode-criv/src/state/store.ts
  - extensions/vscode-criv/src/navigation/references.ts
  - extensions/vscode-criv/src/navigation/languageFeatures.ts
  - extensions/vscode-criv/src/commands/open.ts
  - extensions/vscode-criv/src/extension.ts
  - .obsidian/plugins/criv/src/state/owner.ts
  - .obsidian/plugins/criv/src/source/model.ts
  - .obsidian/plugins/criv/src/source/references.ts
  - .obsidian/plugins/criv/src/main.ts
policy:
  patterns:
    - id: no-editor-node-or-empty-lookup-api
      language: typescript
      pattern: $OBJECT.lookupNode($$$ARGS)
      message: Editor source lookup must use the explicit resolved, unresolved, or ambiguous Wasm result.
    - id: no-direct-obsidian-href-decode
      language: typescript
      rule: |
        all:
          - pattern: decodeURIComponent($TARGET)
          - inside:
              pattern: 'function linkTargets($$$ARGS): $RETURN { $$$BODY }'
              stopBy: end
      message: Obsidian link extraction must use the safe local decoder and contain malformed URI input.
    - id: no-wasm-source-suffix-resolution
      language: rust
      rule: |
        all:
          - pattern: $VALUE.ends_with($QUERY)
          - inside:
              kind: function_item
              has:
                field: name
                regex: ^lookup_source_target$
              stopBy: end
      message: Saved source targets must not resolve through an arbitrary path suffix.
    - id: wasm-lookup-needs-distinct-indexes
      language: rust
      rule: |
        all:
          - kind: function_item
          - has:
              field: name
              regex: ^lookup_source_target$
          - has:
              pattern: self.lookup.legacy.get($KEY)
              stopBy: end
          - not:
              has:
                pattern: self.lookup.exact.get($KEY)
                stopBy: end
      message: Wasm source lookup must test exact identities before the separate legacy alias index.
---

# Retain Explicit Source Lookup Results After the Editor-State Move

## Context

[[0097-explicit-source-target-lookup-results|ADR-0097]] made the `criv-wasm`
loaded revision the only source-target lookup owner, with a tagged `resolved`,
`unresolved`, or `ambiguous` result. Its accepted bytes are immutable.

[[0170-share-editor-state-behaviour-and-embed-the-wasm-runtime|ADR-0170]]
moved the editor lookup adapter into `@criv/editor-state`. ADR-0097 still
governs `packages/criv-editor-state/src/wasmHost.ts`,
`extensions/vscode-criv/src/state/wasm.ts`, and
`.obsidian/plugins/criv/src/state/wasm.ts`. None of these files exist. The
Wasm lookup moved from `crates/criv-wasm/src/lib.rs` to
`crates/criv-wasm/src/source.rs`, and its two indexes are now
`self.lookup.exact` and `self.lookup.legacy`. So the ADR-0097 policies that
name the old index fields or the old adapter functions match no code.

## Decision

Retain the complete decision of ADR-0097. Its rules remain normative by
reference, with these changes only:

- The governed paths are the files that hold the lookup today: the Wasm
  lookup in `crates/criv-wasm/src/source.rs`, the shared adapter in
  `@criv/editor-state`, and the editor hosts that call it.
- The Wasm index policy names `self.lookup.exact` and `self.lookup.legacy`.
  A lookup that reads the legacy index must also read the exact index.
- The `malformed` lookup state is retired. Under ADR-0170 a fragment that is
  not a line range is a symbol selector, so a link target is never malformed
  after lookup. Obsidian still marks a link whose percent encoding does not
  decode.
- The policies that required each result branch in `resolveSourceTarget`,
  `resolveSourceResult`, and `openSourceTarget` are retired. The result is a
  TypeScript discriminated union, so the type checker rejects a caller that
  reads a field of the wrong state. The policy for the removed VS Code
  `parseSourceTarget` fallback is retired with that function; VS Code opens
  only the path of a resolved lookup.
- [[0193-follow-the-cli-source-wikilink-rule-in-editors|ADR-0193]] replaces
  the editor `legacy` diagnostic state.

## Consequences

The governed paths name files that exist, and each retained policy can match
the code it guards. Lookup behaviour does not change.
