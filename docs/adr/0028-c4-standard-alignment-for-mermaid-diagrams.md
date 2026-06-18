---
id: ADR-0028
kind: decision
title: C4 Standard Alignment For Mermaid Diagrams
status: accepted
date: 2026-06-18
supersedes:
  - ADR-0027
governs:
  - README.md
  - src/util.rs
  - src/c4.rs
  - src/vault.rs
  - src/check.rs
  - src/state.rs
  - src/query.rs
  - src/lib.rs
---

# C4 Standard Alignment For Mermaid Diagrams

## Context

[[0027-c4-diagram-fenced-block-extraction-scope|ADR-0027]] clarified the files
governed by Mermaid C4 diagram extraction, while preserving the original
decision from [[0026-mermaid-c4-diagrams-as-vault-content|ADR-0026]].

The first implementation parsed useful Mermaid C4 blocks, but it followed the
Mermaid macro surface more closely than the C4 model vocabulary. Boundary
macros such as `System_Boundary` became architecture elements, graph node kinds
used the diagram level rather than the element category, and relationship labels
were parsed without being exposed in generated state. The README also showed
`c4-code 'src/**'`, which implied a whole-repository class diagram rather than a
focused C4 Code-level view.

C4 diagrams should stay faithful to the model's core abstraction levels: people
use software systems; software systems contain containers; containers contain
components; components are implemented by code elements. Boundaries describe
scope or ownership in a diagram, not architecture elements.

## Decision

Supersede ADR-0027 to align criv's Mermaid C4 support with the C4 standard
vocabulary and level guidance.

`src/c4.rs` distinguishes three parsed construct groups:

- architecture elements: Person, Software System, Container, and Component;
- notation boundaries: `Enterprise_Boundary`, `System_Boundary`,
  `Container_Boundary`, and other `*_Boundary` macros;
- relationships: `Rel*` and `BiRel` macros.

Parsed elements preserve the raw Mermaid macro name for display while also
storing a normalized C4 category. Person and Software System macros treat their
third argument as a description. Container and Component macros treat their
third argument as technology and fourth argument as description.

Boundaries remain parsed as notation metadata, but they are not architecture
elements, do not accept `%% criv:source`, do not become C4 graph element nodes,
and do not satisfy relationship endpoints.

`src/check.rs` validates C4 diagrams by normalized category. `C4Context`
diagrams may contain only people and software systems. `C4Container` diagrams
may contain people, software systems, and containers, but not components.
`C4Component` diagrams may include components and surrounding people, software
systems, or containers. Violations are reported as `invalid-c4-level` errors.

Quality issues that make a diagram less readable but do not necessarily make it
misleading are warnings: missing labels, missing descriptions, missing
Container/Component technology, and missing relationship labels. A
`%% criv:source` comment that follows a boundary, relationship, unknown macro,
or blank/non-construct line is reported as `invalid-c4-source-placement`.

`src/state.rs` writes architecture element nodes by category:
`c4-person`, `c4-software-system`, `c4-container`, and `c4-component`.
Relationship labels are preserved through additive `c4-relationship` nodes with
line-backed paths and `from`/`to` edges to endpoint element nodes. Existing
direct `relates` edges remain for compatibility.

`src/query.rs` exposes the normalized model through `c4-elements`, which now
includes `category=...`, and `c4-relationships`, which lists relationship
labels or `missing`. `c4-code <path-glob>` remains a generated Mermaid
`classDiagram`, but it is documented as a tightly scoped Code-level projection
for a file, component, or module glob. If the glob matches no source files, the
query still emits valid Mermaid with an explicit no-match comment.

## Consequences

Mermaid C4 diagrams in criv docs now better match the original C4 model while
remaining Markdown-native and renderer-free.

Existing diagrams that used boundaries as relationship targets will now fail
validation because boundaries are notation, not architecture elements. Existing
terse diagrams may produce warnings until authors add labels, descriptions,
technology, or relationship labels.

State consumers will see additive `c4-relationship` nodes and more precise C4
element kinds. `STATE_SCHEMA` remains unchanged because the change is additive
and existing node and edge fields are unchanged.

The C4 Code query remains intentionally limited. It can show code elements and
in-scope call edges for a focused area, but it does not infer a complete C4
Component model or generate whole-application architecture diagrams.
