---
name: c4-authoring
description: Author or review the LikeC4 architecture map in docs/architecture. Use for C4 elements, boundaries, named views, view titles, source links, and architecture drift.
metadata:
  criv-template: blake3:8f099f757d561eeb
---

# C4 authoring

Treat `docs/architecture/` as one architecture **map**. LikeC4 source defines
the map. A named view is one **zoom** that tells one story to one audience. The
coding agent owns the meaning; criv validates it; LikeC4 renders it.

## 1. Set the story

Read the complete workspace and the source that the request affects. State the
audience, the question the zoom must answer, and one C4 level:

- System Context — people, the system in scope, and external systems
- Container — the runnable parts of one software system
- Component — the responsibilities inside one container
- Code — selected code elements that implement one important component
- Dynamic — the order of one runtime workflow
- Deployment — which process contains each container instance

Use the least detailed level that answers the question. A Code zoom is optional
and has one component as its scope. The repository programming languages do not
define the view scope.

This step is complete when the audience, question, level, and element in scope
fit in one sentence.

## 2. Trace the real boundary

Use modules, symbols, calls, imports, processes, and stores as evidence. Find
the code and runtime boundaries that implement the story. Then choose clear
architecture names and short responsibilities. A source name becomes an
architecture element only when it is important to the story.

Keep uncertain boundaries explicit in your report. Do not invent a system,
container, component, or relationship that the repository does not prove.

This step is complete when every planned element and relationship has code,
deployment, configuration, or accepted-ADR evidence.

## 3. Change the map

Declare each element and relationship once. Name an element with a noun and
give it one short responsibility. Keep each element at one C4 level:

- A system delivers value to people or other systems.
- A container is an application or data store inside a system.
- A component is a responsibility inside one container.
- A code element helps implement one component.

A store that only this system writes is a container of the system. Hosting is a
deployment fact. At Container level, an adapter depends on its host through the
host API. At Deployment level, the host process contains the adapter instance.

Tag external people and systems with `external` and use the shared external
style. When a Code relationship crosses a component boundary, show the same
dependency at Component level.

A relationship label reads as *Source, label, Destination*: start it with a
capital letter and a present-tense verb, and end it on the object. Add a
`technology` when the relationship crosses a process, language, or storage
boundary.

```likec4
model {
  criv.vscodeExtension -> criv.cli 'Starts check and refresh commands' {
    technology 'Child process'
  }
  validator = component 'Validation engine' {
    link ../../src/check.rs 'source'
  }
}
```

This step is complete when every changed element has one identity, one level,
one responsibility, and all cross-boundary relationships roll up correctly.

## 4. Author the zoom

Select existing map elements with view rules. Use a title that names the level
and scope, such as `Components / CLI` or `Dynamic / State / Publish a
revision`. Make the system or container boundary visible. Include only elements
and relationships that answer the stated question.

Keep a primary named view beside the domain model that it explains. Put
cross-domain workflow views under a focused `views/` folder. A shared
specification or relationship file can own no view. Prefer one primary view per
domain file; add another only when it answers a different question.

Use global style and predicate groups for repeated selections and styles.
Extend a view only when the new zoom is a more detailed form of the base zoom.

This step is complete when the view is readable without spoken explanation and
every included element helps answer the stated question.

## 5. Anchor the map

A `source` link resolves from `docs/architecture/` and accepts a criv file,
line, symbol, or pattern selector. Prefer a stable module or public interface.
Use one link for each implementation boundary that the map claims.

This step is complete when every changed implementation element has a
resolvable source link and no link points outside the boundary it claims.

## 6. Prove the zoom

Run `criv watch --once`, then `criv check`, as the `checking-drift` skill
describes. Open each changed named view in the editor preview. Check its title,
level, boundary, labels, layout, navigation, and source actions.

The work is complete when the vault is green and every changed zoom tells its
stated story with valid source evidence.
