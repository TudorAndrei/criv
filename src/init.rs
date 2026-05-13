use std::fs;
use std::path::Path;

use clap::Args as ClapArgs;

use crate::Result;
use crate::util::{append_line_if_missing, write_new};

#[derive(Debug, Default, ClapArgs)]
pub(crate) struct InitOptions {
    #[arg(long)]
    no_obsidian: bool,
    #[arg(long)]
    no_skills: bool,
}

pub(crate) fn run(root: &Path, options: InitOptions) -> Result<()> {
    let mut created = Vec::new();

    if write_new(&root.join("criv.toml"), DEFAULT_CONFIG)? {
        created.push("criv.toml");
    }

    fs::create_dir_all(root.join("docs/adr"))?;
    fs::create_dir_all(root.join(".criv/snapshots"))?;

    if write_new(&root.join(".criv/state.json"), DEFAULT_STATE)? {
        created.push(".criv/state.json");
    }

    if write_new(&root.join("docs/adr/README.md"), ADR_README)? {
        created.push("docs/adr/README.md");
    }

    if !options.no_skills {
        for (path, contents) in SKILLS {
            if write_new(&root.join(path), contents)? {
                created.push(path);
            }
        }
        for (path, contents) in AGENT_SKILLS {
            if write_new(&root.join(path), contents)? {
                created.push(path);
            }
        }
        for (path, contents) in CLAUDE_SKILLS {
            if write_new(&root.join(path), contents)? {
                created.push(path);
            }
        }
    }

    if !options.no_obsidian {
        for (path, contents) in OBSIDIAN_PLUGIN {
            if write_new(&root.join(path), contents)? {
                created.push(path);
            }
        }
    }

    append_line_if_missing(&root.join(".gitignore"), ".criv/")?;

    if created.is_empty() {
        println!("criv vault already initialized");
    } else {
        println!("initialized criv vault");
        for path in created {
            println!("created {path}");
        }
    }

    Ok(())
}

const DEFAULT_CONFIG: &str = r#"[vault]
docs = "docs"
adr = "adr"

[source]
roots = ["src", "lib"]
exclude = ["**/target/**", "**/node_modules/**"]
languages = []

[index]
source = true
notes = "memory"
embeddings = false

[enforce]
stages = ["commit", "push", "ci"]
# Optional native import policies:
# [[enforce.imports]]
# id = "no-infra-from-ui"
# scope = ["src/ui/**"]
# deny = ["crate::infra::*", "sqlx"]

[obsidian]
plugin = true
"#;

const DEFAULT_STATE: &str = r#"{
  "schema": "criv.state.v0",
  "graph": { "nodes": [], "edges": [] },
  "patterns": {},
  "source-index": []
}
"#;

const ADR_README: &str = r#"---
id: ADR-README
kind: doc
title: Architectural Decisions
tags: [criv]
---

# Architectural Decisions

Accepted decisions live in this directory as MADR-style notes named `NNNN-kebab-title.md`.
"#;

const SKILL_MD: &str = r#"---
id: CRIV-SKILL
kind: doc
title: Working with this criv vault
tags: [criv, skill]
---

# Working with this criv vault

Use this vault to document code, decisions, and references between them.

- Write editorial docs with `kind: doc`.
- Write architectural decisions with `kind: decision` in `docs/adr/`.
- Reference code with wiki-links such as `[[src/lib.rs#some_symbol]]`.
- Reference decisions and docs by `id`, filename, or title.
- Run `criv check` before declaring documentation work complete.

Related skills:

- [[writing-decisions]]
- [[referencing-code]]
- [[checking-drift]]
"#;

const WRITING_DECISIONS: &str = r#"---
id: writing-decisions
kind: doc
title: Writing decisions
tags: [criv, skill]
---

# Writing decisions

Decision notes use `kind: decision`, an ID like `ADR-0001`, and live under `docs/adr/`.

Required fields:

- `id`
- `kind: decision`
- `title`
- `status`
- `date`

Use `governs:` to list path globs controlled by the decision. Use `policy.patterns:` for ast-grep rules that enforcement should evaluate.
"#;

const REFERENCING_CODE: &str = r#"---
id: referencing-code
kind: doc
title: Referencing code
tags: [criv, skill]
---

# Referencing code

Use wiki-links for code, pattern, and note references.

- Source file: `[[src/auth/verify.rs]]`
- Source symbol: `[[src/auth/verify.rs#verify_token]]`
- Source lines: `[[src/auth/verify.rs#L42-L67]]`
- Pattern: `[[match:ADR-0007/no-block-on-in-handler]]`
- Note: `[[ADR-0007]]`

Partial source paths are allowed, but `criv check` warns when they are ambiguous.
"#;

const CHECKING_DRIFT: &str = r#"---
id: checking-drift
kind: doc
title: Checking drift
tags: [criv, skill]
---

# Checking drift

Run `criv check` after editing documentation.

Use `criv check --format json` when an agent or script needs machine-readable diagnostics.
"#;

const SKILLS: &[(&str, &str)] = &[
    ("docs/SKILL.md", SKILL_MD),
    ("docs/skills/writing-decisions.md", WRITING_DECISIONS),
    ("docs/skills/referencing-code.md", REFERENCING_CODE),
    ("docs/skills/checking-drift.md", CHECKING_DRIFT),
];

const AGENT_SKILL_CRIV: &str = r#"---
name: criv
description: Use when working in a criv vault to keep docs, ADRs, source references, checks, state, and enforcement in sync with code changes.
---

# criv

Use `criv` to keep repository documentation connected to source code.

Core workflow:

- Run `criv watch --once` after code or docs changes to refresh `.criv/state.json`.
- Run `criv check` before declaring documentation work complete.
- Use `criv query nodes --kind code --without-docs` to find undocumented code.
- Use `criv query coverage --by module` and `criv query coverage --by adr` to inspect documentation coverage.
- Use `criv enforce --stage ci` before finishing changes that affect ADR-governed code.

Write docs and ADRs with wiki-links to source paths, symbols, patterns, and notes.
"#;

const AGENT_SKILL_WRITING_DECISIONS: &str = r#"---
name: writing-decisions
description: Use when creating or updating criv ADRs under docs/adr with required metadata, governs scopes, and policy patterns.
---

# Writing decisions

Decision notes use `kind: decision`, an ID like `ADR-0001`, and live under `docs/adr/`.

Required fields:

- `id`
- `kind: decision`
- `title`
- `status`
- `date`

Use `governs:` to list path globs controlled by the decision. Use `policy.patterns:` for ast-grep rules that enforcement should evaluate.
"#;

const AGENT_SKILL_REFERENCING_CODE: &str = r#"---
name: referencing-code
description: Use when adding criv wiki-links from docs or ADRs to source files, symbols, line ranges, patterns, and notes.
---

# Referencing code

Use wiki-links for code, pattern, and note references.

- Source file: `[[src/auth/verify.rs]]`
- Source symbol: `[[src/auth/verify.rs#verify_token]]`
- Source lines: `[[src/auth/verify.rs#L42-L67]]`
- Pattern: `[[match:ADR-0007/no-block-on-in-handler]]`
- Note: `[[ADR-0007]]`

Partial source paths are allowed, but `criv check` warns when they are ambiguous.
"#;

const AGENT_SKILL_CHECKING_DRIFT: &str = r#"---
name: checking-drift
description: Use when validating whether criv documentation, ADR metadata, wiki-links, source references, and generated state still match the code.
---

# Checking drift

Run `criv check` after editing documentation.

Run `criv watch --once` after changing code or docs to refresh `.criv/state.json`.

Use `criv check --format json` when an agent or script needs machine-readable diagnostics.
"#;

const AGENT_SKILLS: &[(&str, &str)] = &[
    (".agents/skills/criv/SKILL.md", AGENT_SKILL_CRIV),
    (
        ".agents/skills/writing-decisions/SKILL.md",
        AGENT_SKILL_WRITING_DECISIONS,
    ),
    (
        ".agents/skills/referencing-code/SKILL.md",
        AGENT_SKILL_REFERENCING_CODE,
    ),
    (
        ".agents/skills/checking-drift/SKILL.md",
        AGENT_SKILL_CHECKING_DRIFT,
    ),
];

const CLAUDE_SKILLS: &[(&str, &str)] = &[
    (".claude/skills/criv/SKILL.md", AGENT_SKILL_CRIV),
    (
        ".claude/skills/writing-decisions/SKILL.md",
        AGENT_SKILL_WRITING_DECISIONS,
    ),
    (
        ".claude/skills/referencing-code/SKILL.md",
        AGENT_SKILL_REFERENCING_CODE,
    ),
    (
        ".claude/skills/checking-drift/SKILL.md",
        AGENT_SKILL_CHECKING_DRIFT,
    ),
];

const PLUGIN_MANIFEST: &str = r#"{
  "id": "criv",
  "name": "criv",
  "version": "0.1.0",
  "minAppVersion": "1.5.0",
  "description": "Inline code and pattern references backed by criv state.",
  "author": "criv",
  "isDesktopOnly": true
}
"#;

const PLUGIN_MAIN: &str = include_str!("../.obsidian/plugins/criv/main.js");

const PLUGIN_TS_MAIN: &str = include_str!("../.obsidian/plugins/criv/src/main.ts");

const PLUGIN_TS_WASM: &str = include_str!("../.obsidian/plugins/criv/src/wasm.ts");

const PLUGIN_STYLES: &str = include_str!("../.obsidian/plugins/criv/styles.css");

const PLUGIN_PACKAGE: &str = r#"{
  "name": "criv-obsidian-plugin",
  "version": "0.1.0",
  "description": "Obsidian companion plugin for criv vault state.",
  "main": "main.js",
  "type": "module",
  "scripts": {
    "dev": "node esbuild.config.mjs",
    "build": "npm run build:wasm && tsc -noEmit -skipLibCheck && node esbuild.config.mjs production",
    "build:wasm": "wasm-pack build ../../../crates/criv-wasm --target bundler --out-dir ../../.obsidian/plugins/criv/pkg",
    "version": "node version-bump.mjs && git add manifest.json versions.json",
    "lint": "oxlint src esbuild.config.mjs version-bump.mjs",
    "format": "oxfmt --write src esbuild.config.mjs version-bump.mjs",
    "format:check": "oxfmt --check src esbuild.config.mjs version-bump.mjs"
  },
  "keywords": ["obsidian", "obsidian-plugin", "criv"],
  "license": "MIT",
  "devDependencies": {
    "@types/node": "^16.11.6",
    "esbuild": "0.25.5",
    "oxfmt": "^0.49.0",
    "oxlint": "^1.64.0",
    "tslib": "2.4.0",
    "typescript": "^5.8.3"
  },
  "dependencies": {
    "obsidian": "latest"
  }
}
"#;

const PLUGIN_TSCONFIG: &str = r#"{
  "compilerOptions": {
    "baseUrl": "src",
    "inlineSourceMap": true,
    "inlineSources": true,
    "module": "ESNext",
    "target": "ES6",
    "allowJs": true,
    "noImplicitAny": true,
    "noImplicitThis": true,
    "noImplicitReturns": true,
    "moduleResolution": "node",
    "importHelpers": true,
    "noUncheckedIndexedAccess": true,
    "isolatedModules": true,
    "strictNullChecks": true,
    "strictBindCallApply": true,
    "allowSyntheticDefaultImports": true,
    "useUnknownInCatchVariables": true,
    "lib": ["DOM", "ES5", "ES6", "ES7"]
  },
  "include": ["src/**/*.ts"]
}
"#;

const PLUGIN_ESBUILD: &str = include_str!("../.obsidian/plugins/criv/esbuild.config.mjs");

const PLUGIN_VERSIONS: &str = r#"{
  "0.1.0": "1.5.0"
}
"#;

const PLUGIN_VERSION_BUMP: &str = r#"import { readFileSync, writeFileSync } from "fs";

const targetVersion = process.env.npm_package_version;
const manifest = JSON.parse(readFileSync("manifest.json", "utf8"));
const { minAppVersion } = manifest;
manifest.version = targetVersion;
writeFileSync("manifest.json", JSON.stringify(manifest, null, "\t"));

const versions = JSON.parse(readFileSync("versions.json", "utf8"));
if (!Object.values(versions).includes(minAppVersion)) {
  versions[targetVersion] = minAppVersion;
  writeFileSync("versions.json", JSON.stringify(versions, null, "\t"));
}
"#;

const PLUGIN_LINK_RESOLUTION_FIXTURES: &str = include_str!("../fixtures/link-resolution.json");

const OBSIDIAN_PLUGIN: &[(&str, &str)] = &[
    (".obsidian/plugins/criv/manifest.json", PLUGIN_MANIFEST),
    (".obsidian/plugins/criv/main.js", PLUGIN_MAIN),
    (".obsidian/plugins/criv/styles.css", PLUGIN_STYLES),
    (".obsidian/plugins/criv/src/main.ts", PLUGIN_TS_MAIN),
    (".obsidian/plugins/criv/src/wasm.ts", PLUGIN_TS_WASM),
    (".obsidian/plugins/criv/package.json", PLUGIN_PACKAGE),
    (".obsidian/plugins/criv/tsconfig.json", PLUGIN_TSCONFIG),
    (".obsidian/plugins/criv/esbuild.config.mjs", PLUGIN_ESBUILD),
    (".obsidian/plugins/criv/versions.json", PLUGIN_VERSIONS),
    (
        ".obsidian/plugins/criv/version-bump.mjs",
        PLUGIN_VERSION_BUMP,
    ),
    (
        ".obsidian/plugins/criv/fixtures/link-resolution.json",
        PLUGIN_LINK_RESOLUTION_FIXTURES,
    ),
];
