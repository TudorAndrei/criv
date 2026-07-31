use std::borrow::Cow;
use std::collections::BTreeMap;

use serde::Serialize;

use crate::{CrivError, Result};

pub(crate) struct StaticTemplate {
    pub(crate) path: &'static str,
    pub(crate) contents: &'static str,
}

pub(crate) struct TemplateFile {
    pub(crate) path: &'static str,
    pub(crate) contents: Cow<'static, str>,
}

impl TemplateFile {
    fn borrowed(path: &'static str, contents: &'static str) -> Self {
        Self {
            path,
            contents: Cow::Borrowed(contents),
        }
    }

    fn generated(path: &'static str, contents: String) -> Self {
        Self {
            path,
            contents: Cow::Owned(contents),
        }
    }
}

pub(crate) fn default_config() -> Result<String> {
    let mut toml = toml::to_string_pretty(&DefaultConfig::default())
        .map_err(|err| CrivError::new(format!("failed to serialize default criv.toml: {err}")))?;
    if !toml.ends_with('\n') {
        toml.push('\n');
    }
    Ok(toml)
}

pub(crate) fn default_state() -> Result<String> {
    json_pretty(&DefaultState::default(), ".criv/state.json")
}

pub(crate) fn adr_readme() -> Result<String> {
    let frontmatter = serde_norway::to_string(&AdrReadmeFrontmatter::default()).map_err(|err| {
        CrivError::new(format!(
            "failed to serialize docs/adr/README.md frontmatter: {err}"
        ))
    })?;
    Ok(format!(
        "---\n{}---\n\n# Architectural Decisions\n\nAccepted decisions live in this directory as MADR-style notes named `NNNN-kebab-title.md`.\n",
        frontmatter
    ))
}

pub(crate) fn agent_skills() -> &'static [StaticTemplate] {
    AGENT_SKILLS
}

pub(crate) fn claude_skills() -> &'static [StaticTemplate] {
    CLAUDE_SKILLS
}

/// A stable, compact identity for the source of a generated skill.
pub(crate) fn template_hash(contents: &str) -> String {
    blake3::hash(contents.as_bytes()).to_hex()[..16].to_string()
}

/// Add (or replace) criv's generated-artifact marker in a skill frontmatter
/// block. The embedded templates remain unmarked so their hashes describe the
/// actual shipped skill content.
pub(crate) fn stamped_skill(contents: &str) -> String {
    if skill_marker(contents).is_some() {
        return contents.to_string();
    }
    let marker = format!("criv-template: blake3:{}", template_hash(contents));
    let Some(rest) = contents.strip_prefix("---\n") else {
        return contents.to_string();
    };
    let Some((frontmatter, body)) = rest.split_once("---\n") else {
        return contents.to_string();
    };

    let mut lines: Vec<String> = frontmatter.lines().map(str::to_string).collect();
    if let Some(index) = lines
        .iter()
        .position(|line| line.trim_start() == "metadata:")
    {
        let insert_at = lines[index + 1..]
            .iter()
            .position(|line| !line.starts_with(' ') && !line.starts_with('\t'))
            .map_or(lines.len(), |offset| index + 1 + offset);
        let marker_line = (index + 1..insert_at)
            .find(|&line| lines[line].trim_start().starts_with("criv-template:"));
        if let Some(marker_line) = marker_line {
            lines[marker_line] = format!("  {marker}");
        } else {
            lines.insert(index + 1, format!("  {marker}"));
        }
    } else {
        lines.push("metadata:".to_string());
        lines.push(format!("  {marker}"));
    }

    format!("---\n{}\n---\n{}", lines.join("\n"), body)
}

pub(crate) fn skill_marker(contents: &str) -> Option<&str> {
    let rest = contents.strip_prefix("---\n")?;
    let (frontmatter, _) = rest.split_once("---\n")?;
    let mut in_metadata = false;
    for line in frontmatter.lines() {
        if line.trim_start() == "metadata:" {
            in_metadata = true;
            continue;
        }
        if !line.starts_with(' ') && !line.starts_with('\t') {
            in_metadata = false;
        }
        if in_metadata {
            if let Some(value) = line.trim().strip_prefix("criv-template:") {
                return Some(value.trim());
            }
        }
    }
    None
}

pub(crate) fn pre_commit_hook(repo_relative_root: &str) -> String {
    format!(
        r#"#!/bin/sh
set -eu
cd {}
if command -v criv >/dev/null 2>&1; then
  CRIV_BIN="$(command -v criv)"
elif [ -x ./target/debug/criv ]; then
  CRIV_BIN="./target/debug/criv"
else
  echo "criv hook failed: criv is not on PATH" >&2
  exit 127
fi
"$CRIV_BIN" watch --once
"$CRIV_BIN" check
"$CRIV_BIN" enforce --stage commit
"#,
        shell_quote(repo_relative_root)
    )
}

pub(crate) fn pre_push_hook(repo_relative_root: &str) -> String {
    format!(
        r#"#!/bin/sh
set -eu
cd {}
if command -v criv >/dev/null 2>&1; then
  CRIV_BIN="$(command -v criv)"
elif [ -x ./target/debug/criv ]; then
  CRIV_BIN="./target/debug/criv"
else
  echo "criv hook failed: criv is not on PATH" >&2
  exit 127
fi
"$CRIV_BIN" enforce --stage push --pre-push --remote-name "$1" --remote-url "$2"
"#,
        shell_quote(repo_relative_root)
    )
}

pub(crate) fn obsidian_plugin() -> Result<Vec<TemplateFile>> {
    Ok(vec![
        TemplateFile::generated(
            ".obsidian/app.json",
            json_pretty(&obsidian_app_config(), "Obsidian app.json")?,
        ),
        TemplateFile::generated(
            ".obsidian/plugins/criv/manifest.json",
            json_pretty(&plugin_manifest(), "Obsidian manifest.json")?,
        ),
        TemplateFile::borrowed(".obsidian/plugins/criv/styles.css", PLUGIN_STYLES),
        TemplateFile::borrowed(".obsidian/plugins/criv/src/core.ts", PLUGIN_TS_CORE),
        TemplateFile::borrowed(".obsidian/plugins/criv/src/main.ts", PLUGIN_TS_MAIN),
        TemplateFile::borrowed(".obsidian/plugins/criv/src/wasm.ts", PLUGIN_TS_WASM),
        TemplateFile::generated(
            ".obsidian/plugins/criv/package.json",
            json_pretty(&plugin_package(), "Obsidian package.json")?,
        ),
        TemplateFile::generated(
            ".obsidian/plugins/criv/tsconfig.json",
            json_pretty(&plugin_tsconfig(), "Obsidian tsconfig.json")?,
        ),
        TemplateFile::borrowed(".obsidian/plugins/criv/esbuild.config.mjs", PLUGIN_ESBUILD),
        TemplateFile::generated(
            ".obsidian/plugins/criv/versions.json",
            json_pretty(&plugin_versions(), "Obsidian versions.json")?,
        ),
        TemplateFile::borrowed(
            ".obsidian/plugins/criv/version-bump.mjs",
            PLUGIN_VERSION_BUMP,
        ),
        TemplateFile::borrowed(
            ".obsidian/plugins/criv/fixtures/link-resolution.json",
            PLUGIN_LINK_RESOLUTION_FIXTURES,
        ),
        TemplateFile::borrowed(
            ".obsidian/plugins/criv/test/core.test.mjs",
            PLUGIN_CORE_TEST,
        ),
    ])
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn json_pretty(value: &impl Serialize, label: &str) -> Result<String> {
    let mut json = serde_json::to_string_pretty(value)
        .map_err(|err| CrivError::new(format!("failed to serialize {label}: {err}")))?;
    json.push('\n');
    Ok(json)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ObsidianAppConfig {
    user_ignore_filters: Vec<&'static str>,
}

fn obsidian_app_config() -> ObsidianAppConfig {
    ObsidianAppConfig {
        user_ignore_filters: vec![
            ".criv/",
            ".git/",
            "target/",
            ".obsidian/plugins/criv/node_modules/",
            ".obsidian/plugins/criv/pkg/",
        ],
    }
}

#[derive(Debug, Serialize)]
struct DefaultConfig {
    vault: VaultConfig,
    source: SourceConfig,
    index: IndexConfig,
    enforce: EnforceConfig,
}

impl Default for DefaultConfig {
    fn default() -> Self {
        Self {
            vault: VaultConfig {
                docs: "docs",
                adr: "adr",
            },
            source: SourceConfig {
                roots: vec!["src", "lib"],
                exclude: vec!["**/target/**", "**/node_modules/**"],
            },
            index: IndexConfig {
                source: true,
                embeddings: false,
            },
            enforce: EnforceConfig {
                stages: vec!["commit", "push", "ci"],
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct VaultConfig {
    docs: &'static str,
    adr: &'static str,
}

#[derive(Debug, Serialize)]
struct SourceConfig {
    roots: Vec<&'static str>,
    exclude: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct IndexConfig {
    source: bool,
    embeddings: bool,
}

#[derive(Debug, Serialize)]
struct EnforceConfig {
    stages: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct DefaultState {
    schema: &'static str,
    graph: EmptyGraph,
    patterns: BTreeMap<String, Vec<serde_json::Value>>,
    #[serde(rename = "source-index")]
    source_index: Vec<serde_json::Value>,
}

impl Default for DefaultState {
    fn default() -> Self {
        Self {
            schema: "criv.state.v0",
            graph: EmptyGraph::default(),
            patterns: BTreeMap::new(),
            source_index: Vec::new(),
        }
    }
}

#[derive(Debug, Default, Serialize)]
struct EmptyGraph {
    nodes: Vec<serde_json::Value>,
    edges: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct AdrReadmeFrontmatter {
    id: &'static str,
    kind: &'static str,
    tags: Vec<&'static str>,
}

impl Default for AdrReadmeFrontmatter {
    fn default() -> Self {
        Self {
            id: "ADR-README",
            kind: "doc",
            tags: vec!["criv"],
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginManifest {
    id: &'static str,
    name: &'static str,
    version: &'static str,
    min_app_version: &'static str,
    description: &'static str,
    author: &'static str,
    is_desktop_only: bool,
}

fn plugin_manifest() -> PluginManifest {
    PluginManifest {
        id: "criv",
        name: "criv",
        version: "0.1.0",
        min_app_version: "1.5.0",
        description: "Inline code and pattern references backed by criv state.",
        author: "criv",
        is_desktop_only: true,
    }
}

#[derive(Debug, Serialize)]
struct PluginPackage {
    name: &'static str,
    version: &'static str,
    description: &'static str,
    main: &'static str,
    #[serde(rename = "type")]
    package_type: &'static str,
    scripts: BTreeMap<&'static str, &'static str>,
    keywords: Vec<&'static str>,
    license: &'static str,
    #[serde(rename = "devDependencies")]
    dev_dependencies: BTreeMap<&'static str, &'static str>,
    dependencies: BTreeMap<&'static str, &'static str>,
    #[serde(rename = "allowScripts")]
    allow_scripts: BTreeMap<&'static str, bool>,
}

fn plugin_package() -> PluginPackage {
    PluginPackage {
        name: "criv-obsidian-plugin",
        version: "0.1.0",
        description: "Obsidian companion plugin for criv vault state.",
        main: "main.js",
        package_type: "module",
        scripts: BTreeMap::from([
            (
                "build",
                "npm run build:wasm && tsc -noEmit -skipLibCheck && node esbuild.config.mjs production",
            ),
            (
                "build:wasm",
                "wasm-pack build ../../../crates/criv-wasm --target bundler --out-dir ../../.obsidian/plugins/criv/pkg",
            ),
            ("dev", "node esbuild.config.mjs"),
            (
                "format",
                "oxfmt --write src test esbuild.config.mjs version-bump.mjs",
            ),
            (
                "format:check",
                "oxfmt --check src test esbuild.config.mjs version-bump.mjs",
            ),
            (
                "lint",
                "oxlint src test esbuild.config.mjs version-bump.mjs",
            ),
            ("test", "node test/core.test.mjs"),
            (
                "version",
                "node version-bump.mjs && git add manifest.json versions.json",
            ),
        ]),
        keywords: vec!["obsidian", "obsidian-plugin", "criv"],
        license: "MIT",
        dev_dependencies: BTreeMap::from([
            ("@types/node", "16.18.126"),
            ("esbuild", "0.28.1"),
            ("oxfmt", "0.49.0"),
            ("oxlint", "1.64.0"),
            ("tslib", "2.4.0"),
            ("typescript", "5.8.3"),
        ]),
        dependencies: BTreeMap::from([("obsidian", "1.12.3")]),
        allow_scripts: BTreeMap::from([("esbuild@0.28.1", true)]),
    }
}

#[derive(Debug, Serialize)]
struct PluginTsconfig {
    #[serde(rename = "compilerOptions")]
    compiler_options: CompilerOptions,
    include: Vec<&'static str>,
}

fn plugin_tsconfig() -> PluginTsconfig {
    PluginTsconfig {
        compiler_options: CompilerOptions {
            base_url: "src",
            inline_source_map: true,
            inline_sources: true,
            module: "ESNext",
            target: "ES6",
            allow_js: true,
            no_implicit_any: true,
            no_implicit_this: true,
            no_implicit_returns: true,
            module_resolution: "node",
            import_helpers: true,
            no_unchecked_indexed_access: true,
            isolated_modules: true,
            strict_null_checks: true,
            strict_bind_call_apply: true,
            allow_synthetic_default_imports: true,
            use_unknown_in_catch_variables: true,
            lib: vec!["DOM", "ES5", "ES6", "ES7"],
        },
        include: vec!["src/**/*.ts"],
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompilerOptions {
    base_url: &'static str,
    inline_source_map: bool,
    inline_sources: bool,
    module: &'static str,
    target: &'static str,
    allow_js: bool,
    no_implicit_any: bool,
    no_implicit_this: bool,
    no_implicit_returns: bool,
    module_resolution: &'static str,
    import_helpers: bool,
    no_unchecked_indexed_access: bool,
    isolated_modules: bool,
    strict_null_checks: bool,
    strict_bind_call_apply: bool,
    allow_synthetic_default_imports: bool,
    use_unknown_in_catch_variables: bool,
    lib: Vec<&'static str>,
}

fn plugin_versions() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([("0.1.0", "1.5.0")])
}

const AGENT_SKILL_CRIV: &str = include_str!("../../assets/skills/criv/SKILL.md");
const AGENT_SKILL_CRIV_ME: &str = include_str!("../../assets/skills/criv-me/SKILL.md");
const AGENT_SKILL_WRITING_DECISIONS: &str =
    include_str!("../../assets/skills/writing-decisions/SKILL.md");
const AGENT_SKILL_REFERENCING_CODE: &str =
    include_str!("../../assets/skills/referencing-code/SKILL.md");
const AGENT_SKILL_CHECKING_DRIFT: &str =
    include_str!("../../assets/skills/checking-drift/SKILL.md");
const AGENT_SKILL_C4_AUTHORING: &str = include_str!("../../assets/skills/c4-authoring/SKILL.md");

const AGENT_SKILLS: &[StaticTemplate] = &[
    StaticTemplate {
        path: ".agents/skills/criv/SKILL.md",
        contents: AGENT_SKILL_CRIV,
    },
    StaticTemplate {
        path: ".agents/skills/criv-me/SKILL.md",
        contents: AGENT_SKILL_CRIV_ME,
    },
    StaticTemplate {
        path: ".agents/skills/writing-decisions/SKILL.md",
        contents: AGENT_SKILL_WRITING_DECISIONS,
    },
    StaticTemplate {
        path: ".agents/skills/referencing-code/SKILL.md",
        contents: AGENT_SKILL_REFERENCING_CODE,
    },
    StaticTemplate {
        path: ".agents/skills/checking-drift/SKILL.md",
        contents: AGENT_SKILL_CHECKING_DRIFT,
    },
    StaticTemplate {
        path: ".agents/skills/c4-authoring/SKILL.md",
        contents: AGENT_SKILL_C4_AUTHORING,
    },
];

const CLAUDE_SKILLS: &[StaticTemplate] = &[
    StaticTemplate {
        path: ".claude/skills/criv/SKILL.md",
        contents: AGENT_SKILL_CRIV,
    },
    StaticTemplate {
        path: ".claude/skills/criv-me/SKILL.md",
        contents: AGENT_SKILL_CRIV_ME,
    },
    StaticTemplate {
        path: ".claude/skills/writing-decisions/SKILL.md",
        contents: AGENT_SKILL_WRITING_DECISIONS,
    },
    StaticTemplate {
        path: ".claude/skills/referencing-code/SKILL.md",
        contents: AGENT_SKILL_REFERENCING_CODE,
    },
    StaticTemplate {
        path: ".claude/skills/checking-drift/SKILL.md",
        contents: AGENT_SKILL_CHECKING_DRIFT,
    },
    StaticTemplate {
        path: ".claude/skills/c4-authoring/SKILL.md",
        contents: AGENT_SKILL_C4_AUTHORING,
    },
];

const PLUGIN_TS_CORE: &str = include_str!("../../.obsidian/plugins/criv/src/core.ts");
const PLUGIN_TS_MAIN: &str = include_str!("../../.obsidian/plugins/criv/src/main.ts");
const PLUGIN_TS_WASM: &str = include_str!("../../.obsidian/plugins/criv/src/wasm.ts");
const PLUGIN_STYLES: &str = include_str!("../../.obsidian/plugins/criv/styles.css");
const PLUGIN_ESBUILD: &str = include_str!("../../.obsidian/plugins/criv/esbuild.config.mjs");
const PLUGIN_LINK_RESOLUTION_FIXTURES: &str = include_str!("../../fixtures/link-resolution.json");
const PLUGIN_CORE_TEST: &str = include_str!("../../.obsidian/plugins/criv/test/core.test.mjs");

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

#[cfg(test)]
mod tests {
    use super::*;

    const SKILL: &str = "---\nname: example\ndescription: Example\n---\n\n# Example\n";

    #[test]
    fn template_hash_is_stable_and_content_sensitive() {
        assert_eq!(template_hash(SKILL), template_hash(SKILL));
        assert_ne!(template_hash(SKILL), template_hash("x"));
    }

    #[test]
    fn stamped_skill_has_valid_yaml_and_is_idempotent() {
        let stamped = stamped_skill(SKILL);
        let frontmatter = stamped
            .strip_prefix("---\n")
            .and_then(|value| value.split_once("---\n"))
            .map(|(frontmatter, _)| frontmatter)
            .unwrap();
        serde_norway::from_str::<serde_norway::Value>(frontmatter).unwrap();
        assert_eq!(stamped_skill(&stamped), stamped);
        assert_eq!(
            skill_marker(&stamped),
            Some(format!("blake3:{}", template_hash(SKILL)).as_str())
        );
    }
}
