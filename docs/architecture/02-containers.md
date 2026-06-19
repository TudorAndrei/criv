---
id: architecture-containers
kind: doc
title: criv Container Architecture
---

# criv Container Architecture

This C4 Level 2 view shows the runtime containers inside the `criv` software
system: the Rust CLI, the optional Obsidian companion plugin, and the repository
vault that stores source, docs, configuration, and generated state.

```mermaid
C4Container
title Container diagram for criv
Person(maintainer, "Repository maintainer", "Runs local CLI commands and edits repository files")
System_Ext(git, "Git", "Runs configured hooks and exposes repository history")
System_Ext(obsidian, "Obsidian", "Hosts the companion plugin inside the local vault")
System_Boundary(criv_system, "criv") {
    Container(cli, "criv CLI", "Rust binary", "Owns initialization, validation, query, search, watch, state generation, and enforcement workflows")
    %% criv:source src/main.rs
    Container(plugin, "Obsidian companion plugin", "TypeScript plugin with bundled Rust/WASM helper", "Reads generated state and improves vault editing ergonomics inside Obsidian")
    %% criv:source .obsidian/plugins/criv/src/main.ts
    ContainerDb(vault, "Repository vault", "Markdown, TOML, source files, and JSON state", "Stores docs, ADRs, configuration, source files, policy patterns, and generated .criv state")
    %% criv:source criv.toml
}
Rel(maintainer, cli, "runs shell commands against a local checkout")
Rel(maintainer, vault, "edits source, docs, ADRs, and policy files")
Rel(cli, vault, "reads repository content and writes .criv/state.json snapshots")
Rel(cli, git, "installs hooks and enforces stage-aware repository policy")
Rel(obsidian, plugin, "loads the local plugin in the editor process")
Rel(plugin, vault, "reads .criv/state.json and previews linked source files")
```
