use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::util::read_to_string;
use crate::{CrivError, Result};

#[derive(Debug, Clone)]
pub(crate) struct Config {
    pub(crate) docs_dir: String,
    pub(crate) adr_dir: String,
    pub(crate) source_roots: Vec<String>,
    pub(crate) source_exclude: Vec<String>,
    pub(crate) source_index: bool,
    pub(crate) embeddings: bool,
    pub(crate) architecture_code: Option<ArchitectureCodeConfig>,
    pub(crate) enforce_stages: Vec<String>,
    pub(crate) import_policies: Vec<ImportPolicy>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ArchitectureCodeConfig {
    pub(crate) output: String,
    pub(crate) title: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ImportPolicy {
    pub(crate) id: String,
    pub(crate) scope_matcher: crate::util::GlobMatcher,
    pub(crate) deny: Vec<String>,
    pub(crate) deny_matchers: Vec<Option<crate::util::GlobMatcher>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            docs_dir: "docs".into(),
            adr_dir: "adr".into(),
            source_roots: vec!["src".into(), "lib".into()],
            source_exclude: vec!["**/target/**".into(), "**/node_modules/**".into()],
            source_index: true,
            embeddings: false,
            architecture_code: None,
            enforce_stages: vec!["commit".into(), "push".into(), "ci".into()],
            import_policies: Vec::new(),
        }
    }
}

impl Config {
    pub(crate) fn load(root: &Path) -> Result<Self> {
        let path = root.join("criv.toml");
        if !path.exists() {
            return Ok(Self::default());
        }

        let contents = read_to_string(&path)?;
        let raw: RawConfig = toml::from_str(&contents)
            .map_err(|err| CrivError::new(format!("failed to parse criv.toml: {err}")))?;
        raw.into_config()
    }

    pub(crate) fn docs_path(&self, root: &Path) -> PathBuf {
        root.join(&self.docs_dir)
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawConfig {
    vault: RawVault,
    source: RawSource,
    index: RawIndex,
    architecture: RawArchitecture,
    enforce: RawEnforce,
    patterns: BTreeMap<String, toml::Value>,
}

impl RawConfig {
    fn into_config(self) -> Result<Config> {
        let defaults = Config::default();
        let docs_dir = vault_path("vault.docs", &self.vault.docs.unwrap_or(defaults.docs_dir))?;
        if !self.patterns.is_empty() {
            return Err(CrivError::new(
                "criv.toml [patterns.*] is no longer supported; move persistent patterns into an ADR policy.patterns entry and use its ADR-NNNN/local-id, or use positional structural search with --lang for ad hoc patterns",
            ));
        }
        Ok(Config {
            docs_dir: docs_dir.clone(),
            adr_dir: vault_path("vault.adr", &self.vault.adr.unwrap_or(defaults.adr_dir))?,
            source_roots: self
                .source
                .roots
                .unwrap_or(defaults.source_roots)
                .into_iter()
                .map(|root| vault_path("source.roots", &root))
                .collect::<Result<Vec<_>>>()?,
            source_exclude: self.source.exclude.unwrap_or(defaults.source_exclude),
            source_index: self.index.source.unwrap_or(defaults.source_index),
            embeddings: self.index.embeddings.unwrap_or(defaults.embeddings),
            architecture_code: self
                .architecture
                .code
                .map(|code| code.into_config(&docs_dir))
                .transpose()?,
            enforce_stages: self.enforce.stages.unwrap_or(defaults.enforce_stages),
            import_policies: self
                .enforce
                .imports
                .into_iter()
                .map(RawImportPolicy::into_policy)
                .collect::<Result<Vec<_>>>()?,
        })
    }
}

fn vault_path(field: &str, value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(CrivError::new(format!("{field} must not be empty")));
    }

    let path = Path::new(value);
    if path.is_absolute() {
        return Err(CrivError::new(format!(
            "{field} must be relative to the criv vault root"
        )));
    }

    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(CrivError::new(format!(
                    "{field} must not contain parent-directory segments"
                )));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(CrivError::new(format!(
                    "{field} must be relative to the criv vault root"
                )));
            }
        }
    }

    if parts.is_empty() {
        return Ok(".".into());
    }
    Ok(parts.join("/"))
}

#[derive(Debug, Default, Deserialize)]
struct RawVault {
    docs: Option<String>,
    adr: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawSource {
    roots: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct RawIndex {
    source: Option<bool>,
    embeddings: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawArchitecture {
    code: Option<RawArchitectureCode>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawArchitectureCode {
    output: Option<String>,
    title: Option<String>,
}

impl RawArchitectureCode {
    fn into_config(self, docs_dir: &str) -> Result<ArchitectureCodeConfig> {
        let output = vault_path(
            "architecture.code.output",
            &self
                .output
                .unwrap_or_else(|| format!("{docs_dir}/architecture/04-code.md")),
        )?;
        if !Path::new(&output).starts_with(Path::new(docs_dir)) {
            return Err(CrivError::new(format!(
                "architecture.code.output must be inside vault.docs ({docs_dir})"
            )));
        }
        Ok(ArchitectureCodeConfig {
            output,
            title: self.title.unwrap_or_else(|| "Code diagram for criv".into()),
        })
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawEnforce {
    stages: Option<Vec<String>>,
    imports: Vec<RawImportPolicy>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawImportPolicy {
    id: Option<String>,
    scope: Vec<String>,
    deny: Vec<String>,
}

impl RawImportPolicy {
    fn into_policy(self) -> Result<ImportPolicy> {
        let id = self.id.unwrap_or_else(|| "import-policy".into());
        let scope = if self.scope.is_empty() {
            vec!["**".into()]
        } else {
            self.scope
        };
        let scope_matcher = crate::util::GlobMatcher::new(&scope).map_err(|err| {
            CrivError::new(format!(
                "invalid enforce.imports scope for policy `{id}`: {err}"
            ))
        })?;
        let deny_matchers = self
            .deny
            .iter()
            .map(|deny| {
                (deny.contains('*') || deny.contains('?') || deny.contains('['))
                    .then(|| {
                        crate::util::GlobMatcher::new(&[deny.replace("::", "/")]).map_err(|err| {
                            CrivError::new(format!(
                                "invalid enforce.imports deny glob for policy `{id}`: {err}"
                            ))
                        })
                    })
                    .transpose()
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(ImportPolicy {
            id,
            scope_matcher,
            deny: self.deny,
            deny_matchers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_import_policies() {
        let raw = toml::from_str::<RawConfig>(
            r#"
[enforce]
stages = ["ci"]

[[enforce.imports]]
id = "no-db-from-ui"
scope = ["src/ui/**"]
deny = ["crate::db"]
"#,
        )
        .unwrap();

        let config = raw.into_config().unwrap();
        assert_eq!(config.enforce_stages, vec!["ci"]);
        assert_eq!(config.import_policies.len(), 1);
        assert_eq!(config.import_policies[0].id, "no-db-from-ui");
        assert!(
            config.import_policies[0]
                .scope_matcher
                .is_match("src/ui/view.rs")
        );
        assert_eq!(config.import_policies[0].deny, vec!["crate::db"]);
    }

    #[test]
    fn rejects_invalid_import_policy_scope_and_deny_globs() {
        let scope = toml::from_str::<RawConfig>(
            r#"
[[enforce.imports]]
id = "bad-scope"
scope = ["src/[.rs"]
"#,
        )
        .unwrap()
        .into_config()
        .unwrap_err();
        assert!(scope.to_string().contains("bad-scope"));
        assert!(scope.to_string().contains("scope"));

        let deny = toml::from_str::<RawConfig>(
            r#"
[[enforce.imports]]
id = "bad-deny"
scope = ["src/**"]
deny = ["crate::["]
"#,
        )
        .unwrap()
        .into_config()
        .unwrap_err();
        assert!(deny.to_string().contains("bad-deny"));
        assert!(deny.to_string().contains("deny glob"));
    }

    #[test]
    fn parses_supported_index_config_and_ignores_removed_knobs() {
        let raw = toml::from_str::<RawConfig>(
            r#"
[source]
roots = ["src"]
languages = ["rust"]

[index]
source = false
notes = "memory"
embeddings = true

[obsidian]
plugin = false
"#,
        )
        .unwrap();

        let config = raw.into_config().unwrap();
        assert_eq!(config.source_roots, vec!["src"]);
        assert!(!config.source_index);
        assert!(config.embeddings);
    }

    #[test]
    fn rejects_legacy_pattern_configuration_with_migration_guidance() {
        let error = toml::from_str::<RawConfig>(
            r#"
[patterns.no-println]
language = "rust"
pattern = "println!($$$ARGS)"
"#,
        )
        .unwrap()
        .into_config()
        .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("[patterns.*]"));
        assert!(message.contains("ADR policy.patterns"));
        assert!(message.contains("ADR-NNNN/local-id"));
        assert!(message.contains("--lang"));
    }

    #[test]
    fn parses_architecture_code_config_without_source_glob() {
        let raw = toml::from_str::<RawConfig>(
            r#"
[architecture.code]
output = "docs/architecture/04-code.md"
title = "Code diagram for criv"
"#,
        )
        .unwrap();

        let config = raw.into_config().unwrap();
        assert_eq!(
            config.architecture_code,
            Some(ArchitectureCodeConfig {
                output: "docs/architecture/04-code.md".into(),
                title: "Code diagram for criv".into(),
            })
        );
    }

    #[test]
    fn parses_architecture_code_c4_output_path() {
        let raw = toml::from_str::<RawConfig>(
            r#"
[architecture.code]
output = "docs/architecture/04-code.c4"
title = "Code diagram for criv"
"#,
        )
        .unwrap();

        let config = raw.into_config().unwrap();
        assert_eq!(
            config.architecture_code,
            Some(ArchitectureCodeConfig {
                output: "docs/architecture/04-code.c4".into(),
                title: "Code diagram for criv".into(),
            })
        );
    }

    #[test]
    fn rejects_architecture_code_glob_config() {
        let error = toml::from_str::<RawConfig>(
            r#"
[architecture.code]
output = "docs/architecture/04-code.md"
glob = "src/**"
"#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("unknown field `glob`"));
    }

    #[test]
    fn normalizes_relative_vault_paths() {
        let raw = toml::from_str::<RawConfig>(
            r#"
[vault]
docs = "./docs"
adr = "./adr"

[source]
roots = ["./src", ".github/workflows", "Cargo.toml"]

[architecture.code]
output = "./docs/architecture/04-code.c4"
"#,
        )
        .unwrap();

        let config = raw.into_config().unwrap();
        assert_eq!(config.docs_dir, "docs");
        assert_eq!(config.adr_dir, "adr");
        assert_eq!(
            config.source_roots,
            vec!["src", ".github/workflows", "Cargo.toml"]
        );
        assert_eq!(
            config.architecture_code.unwrap().output,
            "docs/architecture/04-code.c4"
        );
    }

    #[test]
    fn rejects_absolute_vault_paths() {
        let raw = toml::from_str::<RawConfig>(
            r#"
[vault]
docs = "/tmp/docs"
"#,
        )
        .unwrap();

        let error = raw.into_config().unwrap_err();
        assert!(error.to_string().contains("vault.docs"));
        assert!(error.to_string().contains("relative"));
    }

    #[test]
    fn rejects_parent_traversal_in_vault_paths() {
        let raw = toml::from_str::<RawConfig>(
            r#"
[source]
roots = ["src", "../outside"]
"#,
        )
        .unwrap();

        let error = raw.into_config().unwrap_err();
        assert!(error.to_string().contains("source.roots"));
        assert!(error.to_string().contains("parent-directory"));
    }

    #[test]
    fn rejects_empty_vault_paths() {
        let raw = toml::from_str::<RawConfig>(
            r#"
[architecture.code]
output = "  "
"#,
        )
        .unwrap();

        let error = raw.into_config().unwrap_err();
        assert!(error.to_string().contains("architecture.code.output"));
        assert!(error.to_string().contains("must not be empty"));
    }

    #[test]
    fn rejects_architecture_output_outside_docs_directory() {
        let raw = toml::from_str::<RawConfig>(
            r#"
[vault]
docs = "guides"

[architecture.code]
output = "docs/architecture/04-code.md"
"#,
        )
        .unwrap();

        let error = raw.into_config().unwrap_err();
        assert!(error.to_string().contains("architecture.code.output"));
        assert!(error.to_string().contains("vault.docs (guides)"));
    }
}
