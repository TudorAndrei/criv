---
id: likec4-integration
kind: doc
title: Architecture Integration Moves To Concord
---

# Architecture Integration Moves To Concord

[[0155-remove-likec4-from-criv|ADR-0155]] removes
LikeC4 from criv. Architecture and behaviour integration belongs in
[Concord](https://github.com/TudorAndrei/concord), a separate toolkit integrating
criv, LikeC4, and Gherkin. Concord currently contains a scope README, not a
replacement executable or editor preview.

## What remains in criv

criv owns ADR identity, lifecycle, immutability, supersession, governance, and
policy enforcement. Documentation validation, source indexing, links, queries,
and diagnostics support those capabilities. The Obsidian and VS Code companions
retain source navigation, documentation assets, State status, and policy views.
Their source and State features still require the packaged Wasm runtime.

## Breaking removal

criv no longer discovers architecture artifacts, compiles LikeC4, validates
architecture source links, compares architecture interfaces, publishes layout
models, or renders C4 previews. Neither `.c4` nor `.feature` files activate an
integration. Files selected by source roots can still be ordinary indexed
files; criv does not assign them architecture or scenario meaning.

Rebuild generated State after upgrading:

```sh
criv watch --once
criv check
```

New State has no `architecture` field or architecture graph rows. Companions
must be rebuilt with the matching Wasm runtime. They no longer register a C4
editor or preview command. Use a separate LikeC4 tool for existing diagrams;
criv does not delete diagrams in a consuming repository or migrate them into
Concord automatically.

The CLI no longer needs Node.js or LikeC4. Node.js remains development tooling
for the optional editor companions and the documentation site. `criv init`
no longer installs the C4 authoring skill; previously installed architecture
skills are not deleted automatically.

## Historical material

The current `docs/architecture/` files remain repository architecture
documentation. They are not automatically compiled or validated by criv.
Previous runtime implementation and research are available in Git history. The Gherkin proposals in
[issue #198](https://github.com/TudorAndrei/criv/issues/198) need re-triage for
Concord rather than implementation inside criv. There is no automatic
cross-tool integration or scenario-result reporting in this change.
