---
id: ADR-0077
kind: decision
title: C4 Standard Alignment For The LikeC4 Workspace
status: accepted
date: 2026-08-04
governs:
  - .agents/skills/c4-authoring/SKILL.md
  - assets/skills/c4-authoring/SKILL.md
---

# C4 Standard Alignment For The LikeC4 Workspace

## Context

[[0074-likec4-as-the-architecture-source-and-renderer|ADR-0074]] made LikeC4 the
only architecture source. [[0076-focused-likec4-workspace-navigation|ADR-0076]]
split the workspace into folders and gave each view file one primary view.

A review against the C4 model found gaps that ADR-0076 does not cover. The
Overview and Component views still lived inside model files, so the workspace
had no `views/overview/` folder. Every system used one style, so a reader could
not see the boundary of the system in scope. The project repository was a data
store at context level, which mixes two levels in one diagram. Host
applications pointed at criv containers with a "Loads and hosts" label, which
states a deployment fact on a container diagram. The workspace had no
deployment view and no dynamic view. The hand-authored Code model declared
import relations that the component model did not show, so the component
diagram understated coupling. Relationship labels used two styles and never
named a protocol.

## Decision

Model files hold elements and relationships only. View files hold named views
only. Every view lives under `views/overview/`, `views/components/`,
`views/code/`, `views/dynamic/`, or `views/deployment/`.

Tag every external person and system with `external` and grey it in every view.
The system in scope keeps the primary colour.

Keep each element at one level. The project repository is an external software
system. The published state is a `dataStore` container inside criv, because the
CLI writes it and every editor adapter reads it. The shared renderer package is
a container of criv, declared once, and both editor adapters depend on it.

An adapter depends on its host application through the host API. Hosting is a
deployment fact: the deployment model states which process contains each
container instance.

Keep one Component view for each container. Keep a hand-authored Code model
only as a true roll-up of the component model: when a module in component A
imports a module in component B, the component model states a relationship from
A to B. Cross-cutting helper modules, re-export barrels, and bundler shims stay
outside the architecture and are named in a comment.

A relationship label starts with a capital letter and a present-tense verb, and
does not end with a preposition. A relationship carries a `technology` when it
crosses a process, a language, or a storage boundary.

The workspace keeps at least one dynamic view for the refresh session and one
deployment view for the developer workstation.

## Consequences

A reader sees the system boundary, one level for each element, and the order of
the refresh workflow. The container diagram no longer answers deployment
questions, and the deployment diagram answers them instead.

The roll-up rule makes the Code model a maintenance cost that each change must
pay. When that cost is too high, enable `[architecture.code]` and let criv own
the generated Code file instead of keeping the model by hand.

Relationship labels stay readable as *Source, label, Destination*, and protocol
detail appears where a boundary makes it useful.
