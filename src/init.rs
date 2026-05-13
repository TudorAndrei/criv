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

const AGENT_SKILL_CRIV: &str = include_str!("../assets/skills/criv/SKILL.md");
const AGENT_SKILL_CRIV_ME: &str = include_str!("../assets/skills/criv-me/SKILL.md");
const AGENT_SKILL_WRITING_DECISIONS: &str =
    include_str!("../assets/skills/writing-decisions/SKILL.md");
const AGENT_SKILL_REFERENCING_CODE: &str =
    include_str!("../assets/skills/referencing-code/SKILL.md");
const AGENT_SKILL_CHECKING_DRIFT: &str = include_str!("../assets/skills/checking-drift/SKILL.md");

const AGENT_SKILLS: &[(&str, &str)] = &[
    (".agents/skills/criv/SKILL.md", AGENT_SKILL_CRIV),
    (".agents/skills/criv-me/SKILL.md", AGENT_SKILL_CRIV_ME),
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
    (".claude/skills/criv-me/SKILL.md", AGENT_SKILL_CRIV_ME),
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
