use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::repository::RepositoryFiles;
use crate::{CrivError, Result};

#[derive(Debug, Clone)]
pub(crate) struct Config {
    pub(crate) docs_dir: String,
    pub(crate) adr_dir: String,
    pub(crate) source_roots: Vec<String>,
    pub(crate) source_exclude: Vec<String>,
    pub(crate) source_index: bool,
    pub(crate) state_keep: usize,
    pub(crate) enforce_stages: Vec<String>,
    pub(crate) import_policies: Vec<ImportPolicy>,
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
            state_keep: 20,
            enforce_stages: vec!["commit".into(), "push".into(), "ci".into()],
            import_policies: Vec::new(),
        }
    }
}

impl Config {
    #[cfg(test)]
    pub(crate) fn load(root: &Path) -> Result<Self> {
        let files = RepositoryFiles::open(root)?;
        Self::load_from(&files)
    }

    pub(crate) fn load_from(files: &RepositoryFiles) -> Result<Self> {
        let contents = files.read_optional_string(Path::new("criv.toml"))?;
        Self::parse(contents.as_deref())
    }

    /// Parses checkout or Git-tree configuration with the same defaults and
    /// path normalization used by `Config::load`.
    pub(crate) fn parse(contents: Option<&str>) -> Result<Self> {
        let Some(contents) = contents else {
            return Ok(Self::default());
        };
        let raw: RawConfig = toml::from_str(contents)
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
    state: RawState,
    enforce: RawEnforce,
    patterns: BTreeMap<String, toml::Value>,
}

impl RawConfig {
    fn into_config(self) -> Result<Config> {
        let defaults = Config::default();
        let docs_dir = vault_path("vault.docs", &self.vault.docs.unwrap_or(defaults.docs_dir))?;
        if !self.patterns.is_empty() {
            return Err(CrivError::new(
                "criv.toml [patterns.*] is no longer supported; move persistent patterns into an ADR policy.patterns entry and use its ADR-NNNN/local-id",
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
                .map(|root| source_path(&root))
                .collect::<Result<Vec<_>>>()?,
            source_exclude: self.source.exclude.unwrap_or(defaults.source_exclude),
            source_index: self.index.source.unwrap_or(defaults.source_index),
            state_keep: positive_state_keep(self.state.keep.unwrap_or(defaults.state_keep))?,
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

fn source_path(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(CrivError::new("source.roots must not be empty"));
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return Err(CrivError::new(
            "source.roots must be relative to the criv vault root",
        ));
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str().ok_or_else(|| {
                CrivError::new("source.roots must use valid UTF-8 path components")
            })?),
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err(CrivError::new(
                        "source.roots must not escape the criv vault root",
                    ));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(CrivError::new(
                    "source.roots must be relative to the criv vault root",
                ));
            }
        }
    }
    Ok(if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    })
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
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawState {
    keep: Option<usize>,
}

fn positive_state_keep(keep: usize) -> Result<usize> {
    if keep == 0 {
        return Err(CrivError::new("state.keep must be a positive integer"));
    }
    Ok(keep)
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
    fn parses_optional_contents_with_load_defaults_and_normalization() {
        assert_eq!(Config::parse(None).unwrap().docs_dir, "docs");
        let config = Config::parse(Some(
            "[vault]\ndocs = \"./notes\"\nadr = \"./decisions\"\n# comment\n",
        ))
        .unwrap();
        assert_eq!(config.docs_dir, "notes");
        assert_eq!(config.adr_dir, "decisions");
    }

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
legacy = true

[obsidian]
plugin = false
"#,
        )
        .unwrap();

        let config = raw.into_config().unwrap();
        assert_eq!(config.source_roots, vec!["src"]);
        assert!(!config.source_index);
    }

    #[test]
    fn parses_state_retention_and_defaults_to_twenty() {
        assert_eq!(Config::parse(None).unwrap().state_keep, 20);
        assert_eq!(
            Config::parse(Some("[state]\nkeep = 7\n"))
                .unwrap()
                .state_keep,
            7
        );
    }

    #[test]
    fn rejects_zero_and_malformed_state_retention() {
        let zero = Config::parse(Some("[state]\nkeep = 0\n")).unwrap_err();
        assert!(zero.to_string().contains("state.keep"));
        assert!(zero.to_string().contains("positive integer"));

        let malformed = Config::parse(Some("[state]\nkeep = \"many\"\n")).unwrap_err();
        assert!(malformed.to_string().contains("failed to parse criv.toml"));
        assert!(malformed.to_string().contains("keep"));
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
        assert!(error.to_string().contains("escape"));
    }
}
