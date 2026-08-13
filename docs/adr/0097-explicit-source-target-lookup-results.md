---
id: ADR-0097
kind: decision
title: Explicit Source Target Lookup Results
status: accepted
date: 2026-08-12
governs:
  - crates/criv-wasm/src/lib.rs
  - packages/criv-editor-state/src/wasmHost.ts
  - extensions/vscode-criv/src/state/wasm.ts
  - extensions/vscode-criv/src/state/store.ts
  - extensions/vscode-criv/src/navigation/references.ts
  - extensions/vscode-criv/src/navigation/languageFeatures.ts
  - extensions/vscode-criv/src/extension.ts
  - .obsidian/plugins/criv/src/state/wasm.ts
  - .obsidian/plugins/criv/src/source/model.ts
  - .obsidian/plugins/criv/src/main.ts
policy:
  patterns:
    - id: no-editor-node-or-empty-lookup-api
      language: typescript
      pattern: $OBJECT.lookupNode($$$ARGS)
      message: Editor source lookup must use the explicit resolved, unresolved, or ambiguous Wasm result.
    - id: no-vscode-raw-target-open-fallback
      language: typescript
      pattern: parseSourceTarget($NODE?.source_target ?? $NODE?.path ?? $TARGET)
      message: VS Code must not open the raw target after canonical Wasm lookup rejects it.
    - id: no-direct-obsidian-href-decode
      language: typescript
      rule: |
        all:
          - pattern: decodeURIComponent($TARGET)
          - inside:
              pattern: 'function linkTargets($$$ARGS): $RETURN { $$$BODY }'
              stopBy: end
      message: Obsidian link extraction must use the safe local decoder and contain malformed URI input.
    - id: vscode-lookup-adapter-needs-result-states
      language: typescript
      rule: |
        all:
          - pattern: 'export function resolveSourceTarget($$$ARGS): $RETURN { $$$BODY }'
          - any:
              - not:
                  has:
                    pattern: $RESULT.kind === "resolved"
                    stopBy: end
              - not:
                  has:
                    pattern: $RESULT.kind === "unresolved"
                    stopBy: end
              - not:
                  has:
                    pattern: $RESULT.kind === "ambiguous"
                    stopBy: end
      message: The VS Code source lookup adapter must handle every Wasm result state explicitly.
    - id: obsidian-lookup-adapter-needs-result-states
      language: typescript
      rule: |
        all:
          - pattern: 'export function resolveSourceResult($$$ARGS): $RETURN { $$$BODY }'
          - any:
              - not:
                  has:
                    pattern: $RESULT.kind === "resolved"
                    stopBy: end
              - not:
                  has:
                    pattern: $RESULT.kind === "unresolved"
                    stopBy: end
              - not:
                  has:
                    pattern: $RESULT.kind === "ambiguous"
                    stopBy: end
      message: The Obsidian source lookup adapter must handle every Wasm result state explicitly.
    - id: vscode-open-needs-result-states
      language: typescript
      rule: |
        all:
          - pattern: 'async function openSourceTarget($$$ARGS): $RETURN { $$$BODY }'
          - any:
              - not:
                  has:
                    pattern: $RESULT.kind === "resolved"
                    stopBy: end
              - not:
                  has:
                    pattern: $RESULT.kind === "unresolved"
                    stopBy: end
              - not:
                  has:
                    pattern: $RESULT.kind === "ambiguous"
                    stopBy: end
              - not:
                  has:
                    pattern: $RESULT.kind === "malformed"
                    stopBy: end
      message: VS Code source opening must stop on every non-resolved lookup result.
    - id: no-wasm-source-suffix-resolution
      language: rust
      rule: |
        all:
          - pattern: $VALUE.ends_with($QUERY)
          - inside:
              pattern: fn lookup_source_target(&self, $$$ARGS) -> $RETURN { $$$BODY }
              stopBy: end
      message: Saved source targets must not resolve through an arbitrary path suffix.
    - id: wasm-lookup-needs-distinct-indexes
      language: rust
      rule: |
        all:
          - pattern: fn lookup_source_target(&self, $$$ARGS) -> $RETURN { $$$BODY }
          - any:
              - not:
                  has:
                    pattern: self.exact_source_lookup.get($KEY)
                    stopBy: end
              - not:
                  has:
                    pattern: self.legacy_source_lookup.get($KEY)
                    stopBy: end
      message: Wasm source lookup must test exact identities before the separate legacy alias index.
---

# Explicit Source Target Lookup Results

## Context

[[0034-ast-aware-source-selectors|ADR-0034]] makes AST-aware selectors the
canonical source identity and keeps older target forms only for compatibility.
[[0088-share-the-state-wire-document|ADR-0088]] keeps editor lookup in the
loaded Rust-Wasm revision. [[0091-enforce-editor-adapter-boundaries|ADR-0091]]
requires exact lookup, unique legacy aliases, and rejection of an ambiguous
legacy alias.

GitHub issue #104 reported editor-owned first-match rules. Commit `5503043`
removed those rules and moved both editors to Wasm. The remaining node-or-empty
API did not tell an editor whether a target was absent or ambiguous. VS Code
could then parse and open the raw rejected target. Obsidian also decoded local
link targets without containing a malformed percent sequence.

The compatibility boundary needs one result contract. It must not depend on
State order, editor implementation details, fuzzy completion ranking, or an
unsafe path fallback.

## Decision

The active `criv-wasm` loaded revision is the only source-target lookup owner.
Its lookup API returns one tagged result: `resolved`, `unresolved`, or
`ambiguous`. A resolved result contains the canonical target and graph node. An
ambiguous result contains unique candidate records and the complete candidate
count. Each candidate contains the canonical target, node ID, kind, and label.

Test exact identities before compatibility aliases. Exact identities are a
graph-node ID or canonical source target. An exact identity wins when the same
text is a legacy alias for another node. More than one distinct candidate for
the same exact identity is ambiguous. Repeated references to the same candidate
record are one candidate. State order is never a tie-breaker.

Keep only these legacy aliases:

- a file basename;
- a full file path plus an unqualified symbol name; and
- a full file path plus the node label when it differs from the short symbol
  name.

A legacy alias resolves only when it identifies one candidate. Do not resolve
arbitrary suffixes, partial paths, substrings, bare symbol names, changed letter
case, or backslash forms. Fuzzy matching can order canonical completion
choices, but it cannot resolve a stored target.

Only index a node when its canonical target maps to a safe path in the prepared
source index. Both editors keep their own confined path check before file
access. A line fragment such as `#L10-L20` is navigation data. The adapter
validates it, sends the exact file target to Wasm, and applies the range only
after a resolved result. A malformed line fragment is malformed input, not
source identity.

Return at most five ambiguous candidate records, sorted by canonical target and
then node ID, with `total_candidate_count` for the complete set. Editors show
the target and returned candidates as untrusted text. When target text is not
unique, they also show kind, label, and node ID. They do not select a candidate
or open a path until the user selects or writes one canonical target. An editor
can offer one explicit replacement for each displayed candidate, but it must
not rank or apply a replacement without user choice.

Editor adapters remove document syntax such as the `source:` wrapper before
lookup. They keep resolved legacy targets usable and show a migration warning
with the canonical target. They use distinct unresolved, ambiguous, malformed,
and legacy diagnostic states. Full sentences can follow each editor host's UI,
but status meaning, target text, and candidate order stay equal.

Obsidian decodes a local percent-encoded target exactly once. A decode failure
marks only that link as malformed and does not use its undecoded form. It does
not stop work for other links. VS Code cannot parse or open the raw input after
Wasm returns unresolved or ambiguous.

Update the Wasm API, shared editor-state adapter, both editor hosts, generated
Wasm packages, and tests as one change. Do not keep the node-or-empty API as a
second compatibility path. A later package-boundary decision can move neutral
adapter parsing; this decision does not create another TypeScript lookup owner.

Use one versioned fixture for native Rust and both compiled Wasm host tests. It
covers exact targets, unique aliases, symbol and basename collisions, duplicate
exact targets, exact-versus-alias precedence, candidate bounds, graph-order
independence, rejected suffixes, case and separator rejection, and unsafe or
absent paths. Host tests cover `source:` parity, line ranges, command opening,
valid and malformed encoding, diagnostic states, untrusted display, and the
candidate display limit.

## Consequences

Both editors can explain ambiguity without guessing. A rejected target cannot
open an unrelated file, and one malformed link cannot stop editor processing.

The lookup result is larger than an optional node. Ambiguous results have a
fixed transfer bound, and the prepared revision keeps separate exact and legacy
indexes. Completion ranking remains independent from saved-target resolution.

Structural policies prevent the old API, raw-open fallback, direct Obsidian
decode, missing result branches, arbitrary suffix resolution, and index
collapse from returning. Behavioral tests remain the proof of candidate
identity, stable order, safe path mapping, host messages, and file-opening
behavior.
