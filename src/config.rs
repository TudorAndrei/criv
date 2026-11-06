use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use usage_rs::ValueEnum;

use crate::diagnostic::DiagnosticCode;
use crate::glob::GlobMatcher;
use crate::repository::RepositoryFiles;
use crate::{CrivError, Result};

/// The repository-relative path of the vault configuration.
pub const CONFIG_FILE: &str = "criv.toml";

/// Open a repository that holds a vault, which is one with a [`CONFIG_FILE`].
pub fn open_vault(root: &Path) -> Result<RepositoryFiles> {
    let files = RepositoryFiles::open(root)?;
    if files
        .read_optional_string(Path::new(CONFIG_FILE))?
        .is_some()
    {
        return Ok(files);
    }
    Err(CrivError::coded(
        DiagnosticCode::NotAVault,
        format!(
            "not a criv vault: no {CONFIG_FILE} in {}",
            files.root().display()
        ),
    ))
}

#[derive(Debug, Clone)]
pub struct Config {
    pub(crate) docs_dir: String,
    pub(crate) adr_dir: String,
    pub(crate) source_roots: Vec<String>,
    pub(crate) source_exclude: Vec<String>,
    pub(crate) source_index: bool,
    pub(crate) state_keep: usize,
    pub(crate) enforce_stages: Vec<Stage>,
    pub(crate) import_policies: Vec<ImportPolicy>,
}

/// A Git transaction that `criv enforce` validates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Commit,
    Push,
    Ci,
}

impl Stage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Commit => "commit",
            Self::Push => "push",
            Self::Ci => "ci",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImportPolicy {
    pub(crate) id: String,
    pub(crate) scope_matcher: GlobMatcher,
    pub(crate) deny: Vec<DenyRule>,
}

/// One denied import: a module path, or a glob over `/`-separated module paths.
#[derive(Debug, Clone)]
pub struct DenyRule {
    pub(crate) pattern: String,
    matcher: Option<GlobMatcher>,
}

impl DenyRule {
    fn new(pattern: String, policy_id: &str) -> Result<Self> {
        let matcher = pattern
            .contains(['*', '?', '['])
            .then(|| {
                GlobMatcher::new(&[pattern.replace("::", "/")]).map_err(|err| {
                    CrivError::new(format!(
                        "invalid enforce.imports deny glob for policy `{policy_id}`: {err}"
                    ))
                })
            })
            .transpose()?;
        Ok(Self { pattern, matcher })
    }

    /// A module path matches the exact path, any path below it, or the glob.
    pub(crate) fn matches(&self, module: &str) -> bool {
        self.matcher.as_ref().map_or_else(
            || {
                module
                    .strip_prefix(self.pattern.as_str())
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
            },
            |matcher| matcher.is_match(&module.replace("::", "/")),
        )
    }
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
            enforce_stages: vec![Stage::Commit, Stage::Push, Stage::Ci],
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
        let contents = files.read_optional_string(Path::new(CONFIG_FILE))?;
        Self::parse(contents.as_deref())
    }

    /// Parses checkout or Git-tree configuration with the same defaults and
    /// path normalization used by `Config::load`.
    pub(crate) fn parse(contents: Option<&str>) -> Result<Self> {
        let Some(contents) = contents else {
            return Ok(Self::default());
        };
        let raw: RawConfig = toml::from_str(contents)
            .map_err(|err| CrivError::new(format!("failed to parse {CONFIG_FILE}: {err}")))?;
        raw.into_config()
    }

    pub(crate) fn docs_path(&self, root: &Path) -> PathBuf {
        root.join(&self.docs_dir)
    }

    /// The repository-relative ADR directory with a trailing `/`, as Git and
    /// vault paths spell it. A `.` directory adds no segment.
    pub(crate) fn adr_prefix(&self) -> String {
        let mut prefix = String::new();
        for segment in [&self.docs_dir, &self.adr_dir] {
            if segment != "." {
                prefix.push_str(segment);
                prefix.push('/');
            }
        }
        prefix
    }

    /// A decision file: Markdown in the ADR directory, other than its README.
    pub(crate) fn is_adr_path(&self, path: &str) -> bool {
        let prefix = self.adr_prefix();
        path.strip_prefix(&prefix)
            .is_some_and(|name| name != "README.md")
            && Path::new(path)
                .extension()
                .is_some_and(|extension| extension == "md")
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
        let docs_dir = relative_path("vault.docs", &self.vault.docs.unwrap_or(defaults.docs_dir))?;
        if !self.patterns.is_empty() {
            return Err(CrivError::new(format!(
                "{CONFIG_FILE} [patterns.*] is no longer supported; move persistent patterns into an ADR policy.patterns entry and use its ADR-NNNN/local-id",
            )));
        }
        Ok(Config {
            docs_dir,
            adr_dir: relative_path("vault.adr", &self.vault.adr.unwrap_or(defaults.adr_dir))?,
            source_roots: self
                .source
                .roots
                .unwrap_or(defaults.source_roots)
                .into_iter()
                .map(|root| relative_path("source.roots", &root))
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

/// Normalize a configured path to `/`-separated segments below the vault
/// root, or `.` for the root itself.
pub fn relative_path(field: &str, value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(CrivError::new(format!("{field} must not be empty")));
    }
    let mut parts = Vec::new();
    for component in Path::new(value).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str().ok_or_else(|| {
                CrivError::new(format!("{field} must use valid UTF-8 path components"))
            })?),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(CrivError::new(format!(
                    "{field} must not escape the criv vault root with a `..` component"
                )));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(CrivError::new(format!(
                    "{field} must be relative to the criv vault root"
                )));
            }
        }
    }
    Ok(if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    })
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
    stages: Option<Vec<Stage>>,
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
        let scope_matcher = GlobMatcher::new(&scope).map_err(|err| {
            CrivError::new(format!(
                "invalid enforce.imports scope for policy `{id}`: {err}"
            ))
        })?;
        let deny = self
            .deny
            .into_iter()
            .map(|pattern| DenyRule::new(pattern, &id))
            .collect::<Result<Vec<_>>>()?;
        Ok(ImportPolicy {
            id,
            scope_matcher,
            deny,
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
        assert_eq!(config.enforce_stages, vec![Stage::Ci]);
        assert_eq!(config.import_policies.len(), 1);
        assert_eq!(config.import_policies[0].id, "no-db-from-ui");
        assert!(
            config.import_policies[0]
                .scope_matcher
                .is_match("src/ui/view.rs")
        );
        let deny = &config.import_policies[0].deny;
        assert_eq!(deny.len(), 1);
        assert_eq!(deny[0].pattern, "crate::db");
    }

    #[test]
    fn deny_rules_match_exact_prefix_and_glob_forms() {
        let rule = |pattern: &str| DenyRule::new(pattern.into(), "test").unwrap();
        assert!(rule("crate::infra").matches("crate::infra::db"));
        assert!(rule("crate::infra::*").matches("crate::infra::db"));
        assert!(rule("sqlx").matches("sqlx"));
        assert!(!rule("crate::infra").matches("crate::infrastructure"));
    }

    #[test]
    fn adr_prefix_omits_root_directory_segments() {
        let config = Config::parse(Some("[vault]\ndocs = \".\"\n")).unwrap();

        assert_eq!(config.adr_prefix(), "adr/");
        assert!(config.is_adr_path("adr/0001-choice.md"));
        assert!(!config.is_adr_path("adr/README.md"));
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
