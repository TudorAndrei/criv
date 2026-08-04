---
name: c4-authoring
description: Use when authoring or reviewing the LikeC4 architecture workspace in docs/architecture — models, named views, source links, or architecture drift.
---

# C4 authoring

`docs/architecture/` is one LikeC4 workspace. LikeC4 DSL is the only
architecture source, and LikeC4 is the renderer.

## 1. Pick one level

Read the workspace, then choose the level the change belongs to:

- System Context — people, the system in scope, and external systems
- Container — the runnable parts of one software system
- Component — the responsibilities inside one container
- Code — language modules and import relations
- Dynamic — the order of one runtime workflow
- Deployment — which process contains each container instance

Declare each element once, at one level. A store that only this system writes
is a container of the system, not a context-level element. Tag every external
person and system with `external` and grey it in every view, so a reader sees
the system boundary.

Hosting belongs to the deployment model. An adapter depends on its host
application through the host API; a container diagram states dependency, and a
deployment diagram states containment.

LikeC4 merges every source file in the workspace into one model. A domain file
may contain its model declarations and the named views that explain that
domain. This does not copy an element: declare each element and relationship
once, then select it from any view. Keep cross-domain workflow views under
`views/dynamic/` or another focused `views/` folder.

Prefer one primary view for each domain file. A large Code domain may own more
than one focused view when each view answers a different module question. A
shared specification or relationship file may own no view. The editor uses the
LikeC4 `sourcePath` of each named view, so a file preview shows only views that
the file declares.

Done when each changed element has one model identity at one level, and each
planned view has a stated level and the question it answers.

## 2. Author the view

Name an element with a noun, and put a qualifier such as "optional" in the
description or a tag. Select shared elements with LikeC4 view rules rather than
copying model declarations. Put repeated styles and selections in global style
or predicate groups. Extend a view only when the new view is a more detailed
form of the base view.

A relationship label reads as *Source, label, Destination*: start it with a
capital letter and a present-tense verb, and end it on the object rather than a
preposition. Carry a `technology` when the relationship crosses a process, a
language, or a storage boundary.

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

A `source` link resolves from `docs/architecture/` and accepts a criv file,
line, symbol, or pattern selector. Point it at a stable module or public
interface.

Done when every view is readable on its own, every domain file opens its owned
view, and every element that claims an implementation boundary has one
resolvable source link.

## 3. Keep Code architecture at module level

A Code node is a language module: a Rust crate or `mod`, a TypeScript or
JavaScript module or namespace, a Python module or package, a Go package.
Files and symbols are source locations for those modules.

A hand-authored Code model must be a true roll-up of the component model: when
a module in component A imports a module in component B, the component model
states a relationship from A to B. Keep cross-cutting helpers, re-export
barrels, and bundler shims outside the architecture, and name them in a comment
at the top of the Code model file.

When `[architecture.code]` names a Code file, `criv watch --once` owns that
file, and the way to change it is to change source boundaries or generator
behaviour. Enable that setting when the roll-up cost outgrows its value.

Done when every Code node is a language module, every cross-component import
has a component-level relationship, and each view is focused by language or
another bounded module concern.

## 4. Validate

Run `criv watch --once`, then `criv check`, as the `checking-drift` skill
describes. Open each changed view in the editor preview and confirm it renders
the boundary you intended.

The work is complete when the vault is green, every source link resolves, and
every changed view renders its intended boundary.
