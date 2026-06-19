---
id: architecture-context
kind: doc
title: criv System Context Architecture
---

# criv System Context Architecture

This C4 Level 1 view shows `criv` as a local software system used by repository
maintainers and integrated with local Git, optional Obsidian editing, and
optional GitHub Actions workflows.

```mermaid
C4Context
title System Context diagram for criv
Person(maintainer, "Repository maintainer", "Writes source code, docs, ADRs, and policy patterns in a local repository")
System(criv, "criv", "Local docs-to-code knowledge graph validator and query tool")
%% criv:source README.md
System_Ext(git, "Git", "Runs local hooks and provides repository history for enforcement")
System_Ext(obsidian, "Obsidian", "Optional local editor that can load the companion plugin")
System_Ext(github_actions, "GitHub Actions", "Optional CI runner that executes criv checks and release workflows")
Rel(maintainer, criv, "runs CLI commands to initialize, validate, query, search, watch, and enforce the vault")
Rel(criv, git, "installs and runs repository hooks through local Git workflows")
Rel(criv, obsidian, "generates local state consumed by the companion plugin")
Rel(github_actions, criv, "runs the same validation and enforcement commands in CI")
```
