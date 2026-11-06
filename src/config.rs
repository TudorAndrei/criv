use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use usage_rs::ValueEnum;

use crate::diagnostic::DiagnosticCode;
use crate::git::{GitRepository, Revision};
use crate::repository::RepositoryFiles;
use crate::{CrivError, Result};

/// The repository-relative path of the vault configuration.
pub const CONFIG_FILE: &str = "criv.toml";

/// The file name of the decision index in the ADR directory.
const ADR_INDEX: &str = "README.md";

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

/// The vault configuration. Its paths are normalized when it is parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    docs_dir: String,
    adr_dir: String,
    source_roots: Vec<String>,
    source_exclude: Vec<String>,
    source_index: bool,
    state_keep: NonZeroUsize,
    enforce_stages: Vec<Stage>,
    import_policies: Vec<ImportPolicyConfig>,
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

/// One `[[enforce.imports]]` entry as configured.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ImportPolicyConfig {
    pub(crate) id: String,
    pub(crate) scope: Vec<String>,
    pub(crate) deny: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            docs_dir: "docs".into(),
            adr_dir: "adr".into(),
            source_roots: vec!["src".into(), "lib".into()],
            source_exclude: vec!["**/target/**".into(), "**/node_modules/**".into()],
            source_index: true,
            state_keep: NonZeroUsize::MIN.saturating_add(19),
            enforce_stages: vec![Stage::Commit, Stage::Push, Stage::Ci],
            import_policies: Vec::new(),
        }
    }
}

impl Config {
    pub(crate) fn load_from(files: &RepositoryFiles) -> Result<Self> {
        let contents = files.read_optional_string(Path::new(CONFIG_FILE))?;
        Self::parse(contents.as_deref())
    }

    /// The configuration that `revision` holds, or `None` when it has no
    /// [`CONFIG_FILE`].
    pub(crate) fn at_revision(git: &GitRepository, revision: &Revision) -> Result<Option<Self>> {
        let Some(file) = git.file(revision, CONFIG_FILE)? else {
            return Ok(None);
        };
        let contents = String::from_utf8(file.bytes)
            .map_err(|_| CrivError::new(format!("{CONFIG_FILE} at `{revision}` is not UTF-8")))?;
        Self::parse(Some(&contents)).map(Some)
    }

    /// Parses checkout or Git-tree configuration with the same defaults and
    /// path normalization used by `Config::load_from`.
    pub(crate) fn parse(contents: Option<&str>) -> Result<Self> {
        let Some(contents) = contents else {
            return Ok(Self::default());
        };
        let raw: RawConfig = toml::from_str(contents)
            .map_err(|err| CrivError::new(format!("failed to parse {CONFIG_FILE}: {err}")))?;
        raw.into_config()
    }

    /// The documentation directory, `/`-separated below the root, or `.`.
    pub(crate) fn docs_dir(&self) -> &str {
        &self.docs_dir
    }

    pub(crate) fn adr_dir(&self) -> &str {
        &self.adr_dir
    }

    /// The Source roots, each `/`-separated below the root, or `.`.
    pub(crate) fn source_roots(&self) -> &[String] {
        &self.source_roots
    }

    pub(crate) fn source_exclude(&self) -> &[String] {
        &self.source_exclude
    }

    pub(crate) const fn source_index(&self) -> bool {
        self.source_index
    }

    pub(crate) const fn state_keep(&self) -> NonZeroUsize {
        self.state_keep
    }

    pub(crate) fn enforce_stages(&self) -> &[Stage] {
        &self.enforce_stages
    }

    pub(crate) fn import_policies(&self) -> &[ImportPolicyConfig] {
        &self.import_policies
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

    /// The decision index note in the ADR directory.
    pub(crate) fn adr_index_path(&self) -> String {
        format!("{}{ADR_INDEX}", self.adr_prefix())
    }

    /// Whether `other` places decisions in the same directory.
    pub(crate) fn same_decision_scope(&self, other: &Self) -> bool {
        self.docs_dir == other.docs_dir && self.adr_dir == other.adr_dir
    }

    /// A decision file: Markdown in the ADR directory, other than its index.
    pub(crate) fn is_adr_path(&self, path: &str) -> bool {
        let prefix = self.adr_prefix();
        path.strip_prefix(&prefix)
            .is_some_and(|name| name != ADR_INDEX)
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
        let source_roots = self
            .source
            .roots
            .unwrap_or(defaults.source_roots)
            .into_iter()
            .map(|root| relative_path("source.roots", &root))
            .collect::<Result<Vec<_>>>()?;
        Ok(Config {
            docs_dir,
            adr_dir: relative_path("vault.adr", &self.vault.adr.unwrap_or(defaults.adr_dir))?,
            source_roots,
            source_exclude: self.source.exclude.unwrap_or(defaults.source_exclude),
            source_index: self.index.source.unwrap_or(defaults.source_index),
            state_keep: match self.state.keep {
                Some(keep) => NonZeroUsize::new(keep)
                    .ok_or_else(|| CrivError::new("state.keep must be a positive integer"))?,
                None => defaults.state_keep,
            },
            enforce_stages: self.enforce.stages.unwrap_or(defaults.enforce_stages),
            import_policies: self.enforce.imports,
        })
    }
}

/// Normalize a configured path to `/`-separated segments below the vault
/// root, or `.` for the root itself.
fn relative_path(field: &str, value: &str) -> Result<String> {
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
struct RawState {
    keep: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawEnforce {
    stages: Option<Vec<Stage>>,
    imports: Vec<ImportPolicyConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_file_matches_the_editor_fixture() {
        let paths: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/editor/vault-paths.v1.json")).unwrap();
        assert_eq!(paths["config"], CONFIG_FILE);
    }

    /// The shipped template states every default key, so a new `Config`
    /// field does not compile here until it has a template key.
    #[test]
    fn the_init_template_states_every_default() {
        let template = crate::init::templates::DEFAULT_CONFIG;
        assert_eq!(Config::parse(Some(template)).unwrap(), Config::default());

        let Config {
            docs_dir: _,
            adr_dir: _,
            source_roots: _,
            source_exclude: _,
            source_index: _,
            state_keep: _,
            enforce_stages: _,
            // An empty list of `[[enforce.imports]]` tables has no key.
            import_policies: _,
        } = Config::default();
        let keys = [
            "vault.docs",
            "vault.adr",
            "source.roots",
            "source.exclude",
            "index.source",
            "state.keep",
            "enforce.stages",
        ];
        let template: toml::Value = toml::from_str(template).unwrap();
        for key in keys {
            let (table, field) = key.split_once('.').unwrap();
            assert!(
                template
                    .get(table)
                    .and_then(|value| value.get(field))
                    .is_some(),
                "the template must state {key} rather than rely on the default"
            );
        }
    }

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
        let config = Config::parse(Some(
            r#"
[enforce]
stages = ["ci"]

[[enforce.imports]]
id = "no-db-from-ui"
scope = ["src/ui/**"]
deny = ["crate::db"]
"#,
        ))
        .unwrap();

        assert_eq!(config.enforce_stages, vec![Stage::Ci]);
        assert_eq!(
            config.import_policies,
            [ImportPolicyConfig {
                id: "no-db-from-ui".into(),
                scope: vec!["src/ui/**".into()],
                deny: vec!["crate::db".into()],
            }]
        );
    }

    #[test]
    fn an_import_policy_states_its_id_scope_and_deny() {
        for missing in ["id", "scope", "deny"] {
            let entry = [
                ("id", "id = \"policy\""),
                ("scope", "scope = [\"src/**\"]"),
                ("deny", "deny = [\"crate::db\"]"),
            ]
            .into_iter()
            .filter(|(key, _)| *key != missing)
            .map(|(_, line)| line)
            .collect::<Vec<_>>()
            .join("\n");
            let error =
                Config::parse(Some(&format!("[[enforce.imports]]\n{entry}\n"))).unwrap_err();
            assert!(error.to_string().contains(missing), "{error}");
        }
    }

    #[test]
    fn adr_prefix_omits_root_directory_segments() {
        let config = Config::parse(Some("[vault]\ndocs = \".\"\n")).unwrap();

        assert_eq!(config.adr_prefix(), "adr/");
        assert!(config.is_adr_path("adr/0001-choice.md"));
        assert!(!config.is_adr_path("adr/README.md"));
    }

    /// ADR-0021: unknown TOML keys are ignored.
    #[test]
    fn ignores_unknown_configuration_keys() {
        let config = Config::parse(Some(
            r#"
[source]
roots = ["src"]
languages = ["rust"]

[index]
source = false
notes = "memory"

[state]
keep = 3
unknown = true

[obsidian]
plugin = false
"#,
        ))
        .unwrap();

        assert_eq!(config.source_roots, vec!["src"]);
        assert!(!config.source_index);
        assert_eq!(config.state_keep.get(), 3);
    }

    #[test]
    fn parses_state_retention_and_defaults_to_twenty() {
        assert_eq!(Config::parse(None).unwrap().state_keep.get(), 20);
        assert_eq!(
            Config::parse(Some("[state]\nkeep = 7\n"))
                .unwrap()
                .state_keep
                .get(),
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
        let error = Config::parse(Some(
            r#"
[patterns.no-println]
language = "rust"
pattern = "println!($$$ARGS)"
"#,
        ))
        .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("[patterns.*]"));
        assert!(message.contains("ADR policy.patterns"));
        assert!(message.contains("ADR-NNNN/local-id"));
    }

    #[test]
    fn normalizes_relative_vault_paths() {
        let config = Config::parse(Some(
            r#"
[vault]
docs = "./docs"
adr = "./adr"

[source]
roots = ["./src", ".github/workflows", "Cargo.toml"]
"#,
        ))
        .unwrap();

        assert_eq!(config.docs_dir, "docs");
        assert_eq!(config.adr_dir, "adr");
        assert_eq!(
            config.source_roots,
            vec!["src", ".github/workflows", "Cargo.toml"]
        );
    }

    #[test]
    fn rejects_absolute_vault_paths() {
        let error = Config::parse(Some("[vault]\ndocs = \"/tmp/docs\"\n")).unwrap_err();
        assert!(error.to_string().contains("vault.docs"));
        assert!(error.to_string().contains("relative"));
    }

    #[test]
    fn rejects_parent_traversal_in_vault_paths() {
        let error =
            Config::parse(Some("[source]\nroots = [\"src\", \"../outside\"]\n")).unwrap_err();
        assert!(error.to_string().contains("source.roots"));
        assert!(error.to_string().contains("escape"));
    }
}
