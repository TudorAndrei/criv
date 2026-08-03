---
name: c4-authoring
description: LikeC4 architecture authoring and review for criv vaults. Use for .c4 models, named views, source links, generated module views, or architecture drift.
---

# C4 authoring

Treat `docs/architecture/` as one LikeC4 workspace. LikeC4 DSL is the only
architecture source and LikeC4 is the renderer.

## 1. Set the model boundary

Read the existing workspace and choose one C4 level for the change:

- System Context: people, the system in scope, and external systems
- Container: deployable or runnable parts in one software system
- Component: major responsibilities in one container
- Code: language modules and import relations

Add each architectural element to the model once. Use named views to answer
separate review questions.

This step is complete when each changed element has one stable model identity
and each planned view has one stated level and question.

## 2. Author focused views

Give elements readable names and short responsibilities. Give relations labels
with meaningful verbs. Select shared model elements with LikeC4 view rules
instead of copying model declarations.

Use a LikeC4 link titled `source` when an element maps to code:

```likec4
model {
  validator = component 'LikeC4 validator' {
    link ../../src/likec4.rs 'source'
  }
}
```

Resolve the path from `docs/architecture/`. Use a criv file, line, symbol, or
pattern selector. Prefer a stable module or public interface.

This step is complete when every view is readable on its own and every element
that claims an implementation boundary has one resolvable source link.

## 3. Keep Code architecture at module level

Use language-native identities:

- Rust crates and `mod` declarations
- TypeScript and JavaScript modules and namespaces
- Python modules and packages
- Go packages

Files and symbols are source detail. Module identities and import relations are
the Code architecture. `criv watch --once` owns
`docs/architecture/04-code.c4`; change source boundaries or generator behavior
when that file must change.

This step is complete when every Code node is a language module and each view is
focused by language or another bounded module concern.

## 4. Validate the workspace

Run `criv watch --once`, then `criv check`. Inspect LikeC4 model errors,
source-link errors, and the named views in the editor.

The hard cutover has one guardrail: replace Mermaid C4 and DOT artifacts with
LikeC4 source in the same change. Compatibility readers, alternate renderers,
and migration helpers are outside the architecture.

The work is complete when the generated model is current, the full workspace
passes `criv check`, all source links resolve, and every changed named view
renders the intended boundary.
