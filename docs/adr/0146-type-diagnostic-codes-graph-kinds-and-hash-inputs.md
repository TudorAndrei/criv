---
id: ADR-0146
kind: decision
title: Type Diagnostic Codes Graph Kinds and Hash Inputs
status: accepted
date: 2026-10-07
governs:
  - src/diagnostic.rs
  - src/stable_hash.rs
  - src/state.rs
  - src/state/projection.rs
  - src/source/graph.rs
  - crates/criv-state-wire/src/lib.rs
  - fixtures/state/criv.state.v1.json
---

# Type Diagnostic Codes Graph Kinds and Hash Inputs

## Context

criv carried three closed sets as strings.

[[0135-answer-agents-with-codes-fixes-and-next-commands|ADR-0135]] gave each
diagnostic a stable code and a repair. The codes were `&'static str` values,
and `fix_for` matched the same strings in a second place. A new code without a
repair compiled, and a misspelled code compiled too.

The published State graph stored each node kind and edge kind as a `String`.
The source graph already had enums for symbols, directives, and relationships.
The projection turned them into strings, and later code compared those strings
again, for example `node.kind == "architecture-interface"`.

The node, edge, root, and interface hashes hashed formatted text. An absent
value became an empty string, so `None` and `Some("")` produced the same hash.
A field that contained a separator, such as `:` in a type name or `,` in a list
item, could make two different signatures produce the same text.
Partition fingerprints already used length-prefixed input, through free
functions over a `blake3::Hasher`.

## Decision

Model each diagnostic code as a variant of `DiagnosticCode` in
`src/diagnostic.rs`. One exhaustive `match` maps a code to its text, and a
second exhaustive `match` maps it to its repair. A code with no mechanical
repair returns `None` by an explicit arm. `CrivError::Coded` carries a
`DiagnosticCode` and reads the repair from it. This refines the `fix_for`
mechanism of ADR-0135. The codes, the repairs, and the output formats stay as
ADR-0135 describes them.

Model graph kinds as `NodeKind` and `EdgeKind` in the `criv-state-wire` crate.
Their serialized names are the kebab-case names that criv published before.
The variants are in the order of those names, so the derived order sorts edges
as the string order did. A State document that names an unknown kind does not
load.

Hash every published graph value through `StableHasher` in
`src/stable_hash.rs`. Each hash starts with a domain name (`node`, `edge`,
`graph`, or `interface`). A string carries its length, an optional value
carries a presence tag, and a list carries its item count. Partition
fingerprints use the same type. The snapshot hash stays the BLAKE3 hash of the
published bytes, because the store verifies a snapshot by its content.

## Consequences

The compiler lists every site to update when a diagnostic code, a node kind,
or an edge kind is added.

Every node hash, edge hash, graph root, interface hash, and snapshot hash
changes one time. The State shape and the `criv.state.v1` schema do not
change. `fixtures/state/criv.state.v1.json` records the new hashes.

The first `criv check` after an upgrade reports `architecture-interface-drift`
for each LikeC4 interface, because the previous State holds hashes in the old
encoding. `criv watch --once` writes the new hashes and clears the warnings.

A `.criv/state.json` from an earlier criv that holds a retired kind, such as
`c4-interface`, fails to load in `criv check`. `criv watch --once` rebuilds it.
`criv query diff` still reads old snapshots and Git refs, because it reads edge
kinds as text that it only prints.
