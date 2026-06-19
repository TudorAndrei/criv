---
id: architecture-components
kind: doc
title: criv Component Architecture
---

# criv Component Architecture

These C4 Level 3 views split the main containers into readable component
diagrams. The CLI is separated into command-surface and graph-pipeline views so
each diagram stays at one level of abstraction.

## CLI Command Surface

```mermaid
C4Component
title Component diagram for criv CLI command surface
Person(maintainer, "Repository maintainer", "Runs local CLI commands")
System_Ext(git, "Git", "Runs hooks and exposes repository history")
ContainerDb(vault, "Repository vault", "Markdown, TOML, source files, and JSON state", "Stores docs, ADRs, configuration, source files, policy patterns, and generated state")
Container_Boundary(cli_boundary, "criv CLI") {
    Component(cli_dispatch, "CLI dispatch", "Rust and clap", "Parses arguments and routes commands")
    %% criv:source src/lib.rs
    Component(validator, "Validation command", "Rust", "Checks docs, links, source targets, C4 diagrams, ADRs, and policies")
    %% criv:source src/check.rs
    Component(query_engine, "Query command", "Rust", "Answers graph, coverage, C4, source, citation, and diff questions")
    %% criv:source src/query.rs
    Component(search_engine, "Search command", "Rust and fff-search", "Searches files, text, notes, embeddings, and structural rules")
    %% criv:source src/search.rs
    Component(watcher, "Watch command", "Rust and notify", "Refreshes local state after docs or source changes")
    %% criv:source src/watch.rs
    Component(enforcer, "Enforce command", "Rust", "Runs stage-aware commit, push, and CI policy checks")
    %% criv:source src/enforce.rs
    Component(init_generator, "Init command", "Rust", "Creates vault, hook, skill, and plugin scaffolding")
    %% criv:source src/init.rs
}
Rel(maintainer, cli_dispatch, "invokes a CLI command")
Rel(cli_dispatch, validator, "runs check")
Rel(cli_dispatch, query_engine, "runs query")
Rel(cli_dispatch, search_engine, "runs search")
Rel(cli_dispatch, watcher, "runs watch")
Rel(cli_dispatch, enforcer, "runs enforce")
Rel(cli_dispatch, init_generator, "runs init")
Rel(enforcer, git, "checks staged and changed files")
Rel(init_generator, vault, "writes starter files")
```

## CLI Graph Pipeline

```mermaid
C4Component
title Component diagram for criv CLI graph pipeline
ContainerDb(vault, "Repository vault", "Markdown, TOML, source files, and JSON state", "Stores docs, ADRs, configuration, source files, policy patterns, and generated state")
Container_Boundary(cli_boundary, "criv CLI") {
    Component(vault_loader, "Vault loader", "Rust", "Loads config, notes, ADR metadata, wiki-links, source files, C4 diagrams, and source graph data")
    %% criv:source src/vault.rs
    Component(source_graph, "Source graph", "Rust and tree-sitter", "Extracts files, symbols, imports, calls, and attack-surface relationships")
    %% criv:source src/source_graph.rs
    Component(validator, "Validation engine", "Rust", "Validates parsed vault content and resolved source targets")
    %% criv:source src/check.rs
    Component(state_writer, "State writer", "Rust and serde_json", "Serializes graph state, pattern matches, source index, and snapshots")
    %% criv:source src/state.rs
    Component(query_engine, "Query engine", "Rust", "Answers questions from loaded vault state")
    %% criv:source src/query.rs
    Component(search_engine, "Search engine", "Rust and fff-search", "Searches configured source and note content")
    %% criv:source src/search.rs
    Component(watcher, "Watch loop", "Rust and notify", "Triggers incremental rebuilds after file changes")
    %% criv:source src/watch.rs
}
Rel(vault_loader, source_graph, "builds source graph")
Rel(validator, vault_loader, "checks loaded vault")
Rel(state_writer, vault_loader, "serializes loaded vault")
Rel(query_engine, vault_loader, "queries loaded vault")
Rel(search_engine, vault_loader, "searches loaded vault")
Rel(watcher, state_writer, "writes changed snapshots")
Rel(state_writer, vault, "writes .criv state")
```

## Obsidian Companion Plugin

```mermaid
C4Component
title Component diagram for Obsidian companion plugin container
System_Ext(obsidian, "Obsidian", "Hosts the local plugin and provides vault and editor APIs")
ContainerDb(vault, "Repository vault", "Markdown, TOML, source files, and JSON state", "Stores docs, ADRs, configuration, source files, policy patterns, and generated .criv state")
Container_Boundary(plugin_boundary, "Obsidian companion plugin") {
    Component(plugin_runtime, "Plugin runtime", "TypeScript and Obsidian API", "Registers commands, views, hover previews, editor suggestions, and state reload behavior")
    %% criv:source .obsidian/plugins/criv/src/main.ts
    Component(core_helpers, "Link and state helpers", "TypeScript", "Resolves criv state links, source suggestions, link ranges, pattern targets, and tooltips")
    %% criv:source .obsidian/plugins/criv/src/core.ts
    Component(wasm_bridge, "WASM bridge", "TypeScript", "Loads the generated criv-wasm package for plugin-side helper calls")
    %% criv:source .obsidian/plugins/criv/src/wasm.ts
    Component(wasm_helper, "criv-wasm helper", "Rust compiled to WebAssembly", "Summarizes generated criv state for plugin status and compatibility checks")
    %% criv:source crates/criv-wasm/src/lib.rs
}
Rel(obsidian, plugin_runtime, "loads and calls plugin lifecycle hooks")
Rel(plugin_runtime, vault, "reads .criv/state.json and source files through the Obsidian vault adapter")
Rel(plugin_runtime, core_helpers, "uses shared link parsing and tooltip helpers")
Rel(plugin_runtime, wasm_bridge, "calls state summary helpers")
Rel(wasm_bridge, wasm_helper, "loads wasm-bindgen exports")
Rel(core_helpers, vault, "interprets graph nodes, source paths, and pattern data from generated state")
```
