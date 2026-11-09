//! File discovery profiles and their shared traversal rules.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use criv_state_wire::{asset, layout};
use ignore::{DirEntry, Error as WalkError, ParallelVisitor, ParallelVisitorBuilder, WalkBuilder};

use crate::config::Config;
use crate::glob::GlobMatcher;
use crate::repository::RepositoryFiles;
use crate::{CrivError, Result};

const VAULT_WALK_THREADS: usize = 8;
const COLLECT_FLUSH_ENTRIES: usize = 1_024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultPaths {
    pub(crate) markdown: Vec<String>,
    pub(crate) assets: Vec<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
enum SelectionKind {
    Source,
    VaultMarkdown,
    VaultAsset,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct Selection {
    kind: SelectionKind,
    path: String,
}

#[derive(Debug, Default)]
struct Collected {
    selections: Vec<Selection>,
    errors: Vec<String>,
}

#[derive(Debug, Clone)]
enum Profile {
    Source {
        exclude: GlobMatcher,
        prune: Option<GlobMatcher>,
    },
    Vault,
}

impl Profile {
    fn prunes_directory(&self, relative: &str, name: &str) -> bool {
        match self {
            Self::Source { prune, .. } => {
                matches!(name, ".git" | layout::DIR)
                    || prune.as_ref().is_some_and(|set| set.is_match(relative))
            }
            Self::Vault => matches!(name, ".git" | layout::DIR | "target" | "node_modules"),
        }
    }

    fn candidate_kind(&self, path: &Path) -> Option<SelectionKind> {
        match self {
            Self::Source { .. } => Some(SelectionKind::Source),
            Self::Vault => vault_selection_kind(path),
        }
    }

    fn selects(&self, relative: &str) -> bool {
        match self {
            Self::Source { exclude, .. } => !exclude.is_match(relative),
            Self::Vault => true,
        }
    }
}

/// The configured Source selection: whether Source indexing runs, its
/// normalized roots, and its exclusion rules. The candidate walk and the
/// watch event filter read the same scope.
#[derive(Debug, Clone)]
pub struct SourceScope {
    rules: Option<SourceRules>,
}

#[derive(Debug, Clone)]
struct SourceRules {
    roots: Vec<String>,
    profile: Arc<Profile>,
}

impl SourceScope {
    pub(crate) fn new(config: &Config) -> Result<Self> {
        if !config.source_index() {
            return Ok(Self { rules: None });
        }
        let roots = config
            .source_roots()
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let profile = Profile::Source {
            exclude: GlobMatcher::new(config.source_exclude())?,
            prune: subtree_prune_matcher(config.source_exclude())?,
        };
        Ok(Self {
            rules: Some(SourceRules {
                roots,
                profile: Arc::new(profile),
            }),
        })
    }

    pub(crate) const fn is_enabled(&self) -> bool {
        self.rules.is_some()
    }
}

pub fn discover_source_candidates(
    files: &RepositoryFiles,
    scope: &SourceScope,
) -> Result<Vec<String>> {
    let Some(rules) = &scope.rules else {
        return Ok(Vec::new());
    };
    let root = files.root();
    let plan = SourcePlan::new(root, &rules.roots, &rules.profile)?;
    let mut collected = if plan.directories.is_empty() {
        Collected::default()
    } else {
        let mut builder = WalkBuilder::empty();
        for directory in &plan.directories {
            builder.add(root.join(directory));
        }
        builder.standard_filters(false).follow_links(false);
        walk(root, &mut builder, rules.profile.clone())
    };

    for file in plan.files {
        collect_explicit_source(root, &file, &rules.profile, &mut collected);
    }
    finish(collected).map(|selections| {
        selections
            .into_iter()
            .map(|selection| selection.path)
            .collect()
    })
}

#[derive(Debug)]
pub struct SourceEventFilter {
    root: PathBuf,
    scope: SourceScope,
}

impl SourceEventFilter {
    pub(crate) fn new(root: &Path, config: &Config) -> Result<Self> {
        Ok(Self {
            root: root.to_path_buf(),
            scope: SourceScope::new(config)?,
        })
    }

    /// Whether a change at `event` can change the Source selection or a
    /// selected file. A path that is not valid UTF-8 is judged by its lossy
    /// form, so one under a root still refreshes and reports the bad path.
    pub(crate) fn relevant(&self, event: &Path) -> bool {
        let Some(rules) = &self.scope.rules else {
            return false;
        };
        let Ok(relative) = event.strip_prefix(&self.root) else {
            return false;
        };
        let Some(relative) = join_normal_components(relative) else {
            return true;
        };
        let intersects_root = rules.roots.iter().any(|source_root| {
            path_contains(source_root, &relative) || path_contains(&relative, source_root)
        });
        intersects_root
            && !profile_prunes_path(&rules.profile, &relative)
            && rules.profile.selects(&relative)
    }
}

/// Decides which watch events can change the vault selection, by the prune
/// rules of the vault walk.
#[derive(Debug)]
pub struct VaultEventFilter {
    root: PathBuf,
    docs: String,
}

impl VaultEventFilter {
    pub(crate) fn new(root: &Path, config: &Config) -> Self {
        Self {
            root: root.to_path_buf(),
            docs: config.docs_dir().to_string(),
        }
    }

    /// Whether a change at `event` is below the docs root and outside every
    /// directory that the vault walk prunes. A path that is not valid UTF-8
    /// is judged by its lossy form.
    pub(crate) fn relevant(&self, event: &Path) -> bool {
        let Ok(relative) = event.strip_prefix(&self.root) else {
            return false;
        };
        let Some(relative) = join_normal_components(relative) else {
            return true;
        };
        path_contains(&self.docs, &relative) && !profile_prunes_path(&Profile::Vault, &relative)
    }
}

/// The notes and assets under `docs_dir`, as [`Config::docs_dir`] normalizes it.
pub fn discover_vault(root: &Path, docs_relative: &str) -> Result<VaultPaths> {
    let profile = Arc::new(Profile::Vault);
    if profile_prunes_path(profile.as_ref(), docs_relative) {
        return Ok(VaultPaths {
            markdown: Vec::new(),
            assets: Vec::new(),
        });
    }
    let Some(kind) = validate_root(root, docs_relative)? else {
        return Ok(VaultPaths {
            markdown: Vec::new(),
            assets: Vec::new(),
        });
    };
    if kind != RootKind::Directory {
        return Err(CrivError::new(format!(
            "vault walk root `{docs_relative}` must be a real directory"
        )));
    }

    let mut builder = WalkBuilder::new(root.join(docs_relative));
    builder
        .standard_filters(false)
        .follow_links(false)
        .threads(VAULT_WALK_THREADS);
    let selections = finish(walk(root, &mut builder, profile))?;
    let mut markdown = Vec::new();
    let mut assets = Vec::new();
    for selection in selections {
        match selection.kind {
            SelectionKind::VaultMarkdown => markdown.push(selection.path),
            SelectionKind::VaultAsset => assets.push(selection.path),
            SelectionKind::Source => {}
        }
    }
    Ok(VaultPaths { markdown, assets })
}

fn vault_selection_kind(path: &Path) -> Option<SelectionKind> {
    let extension = path.extension()?.to_str()?;
    if extension == "md" {
        return Some(SelectionKind::VaultMarkdown);
    }
    asset::mime_for_path(path.to_str()?).map(|_| SelectionKind::VaultAsset)
}

fn walk(root: &Path, builder: &mut WalkBuilder, profile: Arc<Profile>) -> Collected {
    let root = Arc::new(root.to_path_buf());
    let filter_root = root.clone();
    let filter_profile = profile.clone();
    builder.filter_entry(move |entry| {
        if entry.depth() == 0 {
            return true;
        }
        let Some(file_type) = entry.file_type() else {
            return true;
        };
        if !file_type.is_dir() {
            return true;
        }
        let Ok(relative) = relative_utf8(&filter_root, entry.path()) else {
            return true;
        };
        let Some(name) = entry.file_name().to_str() else {
            return true;
        };
        !filter_profile.prunes_directory(&relative, name)
    });

    let shared = Arc::new(Mutex::new(Collected::default()));
    let mut visitors = CollectorBuilder {
        root,
        profile,
        shared: shared.clone(),
    };
    builder.build_parallel().visit(&mut visitors);
    std::mem::take(
        &mut *shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

struct CollectorBuilder {
    root: Arc<PathBuf>,
    profile: Arc<Profile>,
    shared: Arc<Mutex<Collected>>,
}

impl<'scope> ParallelVisitorBuilder<'scope> for CollectorBuilder {
    fn build(&mut self) -> Box<dyn ParallelVisitor + 'scope> {
        Box::new(Collector {
            root: self.root.clone(),
            profile: self.profile.clone(),
            shared: self.shared.clone(),
            local: Collected::default(),
        })
    }
}

struct Collector {
    root: Arc<PathBuf>,
    profile: Arc<Profile>,
    shared: Arc<Mutex<Collected>>,
    local: Collected,
}

impl ParallelVisitor for Collector {
    fn visit(&mut self, entry: std::result::Result<DirEntry, WalkError>) -> ignore::WalkState {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                self.local
                    .errors
                    .push(normalize_walk_error(&self.root, &error));
                return ignore::WalkState::Continue;
            }
        };
        if let Some(error) = entry.error() {
            self.local
                .errors
                .push(normalize_walk_error(&self.root, error));
        }
        if entry.depth() == 0 && entry.path() == self.root.as_path() {
            return ignore::WalkState::Continue;
        }
        self.collect_entry(&entry);
        if self.local.selections.len() >= COLLECT_FLUSH_ENTRIES {
            self.flush();
        }
        ignore::WalkState::Continue
    }
}

impl Collector {
    fn flush(&mut self) {
        let mut shared = self
            .shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        shared.selections.append(&mut self.local.selections);
        shared.errors.append(&mut self.local.errors);
    }

    fn collect_entry(&mut self, entry: &DirEntry) {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Some(file_type) => file_type,
            None => match fs::symlink_metadata(path) {
                Ok(metadata) => metadata.file_type(),
                Err(error) => {
                    self.local.errors.push(path_error(&self.root, path, &error));
                    return;
                }
            },
        };
        let is_link = entry.path_is_symlink() || file_type.is_symlink() || is_junction(path);
        let target_is_directory =
            is_link && fs::metadata(path).is_ok_and(|target_metadata| target_metadata.is_dir());
        if target_is_directory {
            let relative = match relative_utf8(&self.root, path) {
                Ok(relative) => relative,
                Err(error) => {
                    self.local.errors.push(error.to_string());
                    return;
                }
            };
            if profile_prunes_path(self.profile.as_ref(), &relative) {
                return;
            }
            self.local
                .errors
                .push(format!("refusing to discover directory link `{relative}`"));
            return;
        }
        if file_type.is_dir() {
            if let Err(error) = relative_utf8(&self.root, path) {
                self.local.errors.push(error.to_string());
            }
            return;
        }

        let Some(kind) = self.profile.candidate_kind(path) else {
            return;
        };
        let relative = match relative_utf8(&self.root, path) {
            Ok(relative) if !relative.is_empty() => relative,
            Ok(_) => return,
            Err(error) => {
                self.local.errors.push(error.to_string());
                return;
            }
        };
        if !self.profile.selects(&relative) {
            return;
        }
        if is_link {
            self.local
                .errors
                .push(format!("refusing to discover file link `{relative}`"));
            return;
        }
        if !file_type.is_file() {
            return;
        }
        self.local.selections.push(Selection {
            kind,
            path: relative,
        });
    }
}

impl Drop for Collector {
    fn drop(&mut self) {
        self.flush();
    }
}

fn finish(mut collected: Collected) -> Result<Vec<Selection>> {
    collected.errors.sort();
    collected.errors.dedup();
    if !collected.errors.is_empty() {
        return Err(CrivError::new(format!(
            "file discovery failed:\n{}",
            collected
                .errors
                .into_iter()
                .map(|error| format!("- {error}"))
                .collect::<Vec<_>>()
                .join("\n")
        )));
    }
    collected.selections.sort();
    collected.selections.dedup();
    Ok(collected.selections)
}

fn collect_explicit_source(
    root: &Path,
    relative: &str,
    profile: &Profile,
    collected: &mut Collected,
) {
    if !profile.selects(relative) {
        return;
    }
    let path = root.join(relative);
    if is_junction(&path)
        || fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        collected
            .errors
            .push(format!("refusing to discover file link `{relative}`"));
        return;
    }
    collected.selections.push(Selection {
        kind: SelectionKind::Source,
        path: relative.to_string(),
    });
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum RootKind {
    Directory,
    File,
}

#[derive(Debug)]
struct SourcePlan {
    directories: Vec<String>,
    files: Vec<String>,
}

impl SourcePlan {
    fn new(root: &Path, roots: &[String], profile: &Profile) -> Result<Self> {
        let mut directories = Vec::new();
        let mut files = Vec::new();
        for relative in roots {
            if profile_prunes_path(profile, relative) {
                continue;
            }
            match validate_root(root, relative)? {
                Some(RootKind::Directory) => directories.push(relative.clone()),
                Some(RootKind::File) => files.push(relative.clone()),
                None => {}
            }
        }
        directories.sort_by(|left, right| {
            component_count(left)
                .cmp(&component_count(right))
                .then_with(|| left.cmp(right))
        });
        let mut reduced: Vec<String> = Vec::new();
        for directory in directories {
            if !reduced
                .iter()
                .any(|ancestor| path_contains(ancestor, &directory))
            {
                reduced.push(directory);
            }
        }
        files.retain(|file| {
            !reduced
                .iter()
                .any(|directory| path_contains(directory, file))
        });
        files.sort();
        Ok(Self {
            directories: reduced,
            files,
        })
    }
}

fn profile_prunes_path(profile: &Profile, relative: &str) -> bool {
    if relative == "." {
        return false;
    }
    let mut prefix = String::new();
    for component in relative.split('/') {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(component);
        if profile.prunes_directory(&prefix, component) {
            return true;
        }
    }
    false
}

fn validate_root(root: &Path, relative: &str) -> Result<Option<RootKind>> {
    let mut current = root.to_path_buf();
    let components = Path::new(relative).components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        if let Component::Normal(value) = component {
            current.push(value);
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || is_junction(&current) => {
                return Err(CrivError::new(format!(
                    "discovery root `{relative}` contains a link at `{}`",
                    display_relative(root, &current)
                )));
            }
            Ok(metadata) if index.saturating_add(1) < components.len() && !metadata.is_dir() => {
                return Err(CrivError::new(format!(
                    "discovery root `{relative}` has a non-directory component `{}`",
                    display_relative(root, &current)
                )));
            }
            Ok(metadata) if index.saturating_add(1) == components.len() => {
                return if metadata.is_dir() {
                    Ok(Some(RootKind::Directory))
                } else if metadata.is_file() {
                    Ok(Some(RootKind::File))
                } else {
                    Ok(None)
                };
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(Some(RootKind::Directory))
}

fn relative_utf8(root: &Path, path: &Path) -> Result<String> {
    let relative = path.strip_prefix(root).map_err(|_| {
        CrivError::new(format!(
            "discovered path {} is outside repository root {}",
            path.display(),
            root.display()
        ))
    })?;
    if relative.to_str().is_none() {
        return Err(CrivError::new(format!(
            "discovered path {} is not valid UTF-8",
            path.display()
        )));
    }
    join_normal_components(relative).ok_or_else(|| {
        CrivError::new(format!(
            "discovered path {} has an invalid repository identity",
            path.display()
        ))
    })
}

/// The `/`-joined normal components of `relative`, or `None` when a
/// component leaves it. A component that is not UTF-8 joins in lossy form.
fn join_normal_components(relative: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(parts.join("/"))
}

fn subtree_prune_matcher(patterns: &[String]) -> Result<Option<GlobMatcher>> {
    let prefixes = patterns
        .iter()
        .filter_map(|pattern| pattern.strip_suffix("/**"))
        .filter(|prefix| !prefix.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    (!prefixes.is_empty())
        .then(|| GlobMatcher::new(&prefixes))
        .transpose()
}

fn component_count(path: &str) -> usize {
    if path == "." {
        0
    } else {
        path.split('/').count()
    }
}

fn path_contains(ancestor: &str, path: &str) -> bool {
    ancestor == "."
        || path == ancestor
        || path
            .strip_prefix(ancestor)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn normalize_walk_error(root: &Path, error: &WalkError) -> String {
    error
        .to_string()
        .replace(&root.to_string_lossy().to_string(), ".")
}

fn path_error(root: &Path, path: &Path, error: &std::io::Error) -> String {
    format!(
        "failed to inspect `{}`: {error}",
        display_relative(root, path)
    )
}

fn display_relative(root: &Path, path: &Path) -> String {
    relative_utf8(root, path).unwrap_or_else(|_| path.display().to_string())
}

#[cfg(windows)]
fn is_junction(path: &Path) -> bool {
    junction::exists(path).unwrap_or(false)
}

#[cfg(not(windows))]
const fn is_junction(_path: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(path: &Path, contents: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    /// A configuration with this `[source]` table.
    fn source_config(source: &str) -> Config {
        Config::parse(Some(&format!("[source]\n{source}\n"))).unwrap()
    }

    fn discover_source(root: &Path, config: &Config) -> Result<Vec<String>> {
        discover_source_candidates(&RepositoryFiles::open(root)?, &SourceScope::new(config)?)
    }

    fn event_relevant(root: &Path, config: &Config, event: &Path) -> bool {
        SourceEventFilter::new(root, config)
            .unwrap()
            .relevant(event)
    }

    #[test]
    fn source_uses_roots_excludes_and_hidden_rules_without_reading_contents() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write(&root.join("src/lib.rs"), b"pub fn lib() {}\n");
        write(&root.join("src/.hidden.rs"), b"pub fn hidden() {}\n");
        write(&root.join("src/ignored.rs"), b"pub fn ignored() {}\n");
        write(&root.join("src/.gitignore"), b"ignored.rs\n");
        write(&root.join("src/excluded.rs"), b"pub fn excluded() {}\n");
        write(&root.join("src/binary.rs"), b"\0binary");
        write(&root.join(".git/private.rs"), b"private\n");
        let config =
            source_config("roots = [\"src\", \"./src/.\"]\nexclude = [\"src/excluded.rs\"]");

        assert_eq!(
            discover_source(root, &config).unwrap(),
            vec![
                "src/.gitignore".to_string(),
                "src/.hidden.rs".to_string(),
                "src/binary.rs".to_string(),
                "src/ignored.rs".to_string(),
                "src/lib.rs".to_string(),
            ]
        );
    }

    #[test]
    fn an_invalid_exclude_glob_fails_the_walk_and_the_event_filter() {
        let temp = TempDir::new().unwrap();
        let config = source_config("roots = [\"src\"]\nexclude = [\"src/[\"]");

        assert!(discover_source(temp.path(), &config).is_err());
        assert!(SourceEventFilter::new(temp.path(), &config).is_err());
    }

    #[test]
    fn source_discovers_elixir_directory_explicit_root_and_events() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write(&root.join("lib/sample.ex"), b"defmodule Sample do\nend\n");
        write(
            &root.join("scripts/check.exs"),
            b"defmodule Check do\nend\n",
        );
        let config = source_config("roots = [\"lib\", \"scripts/check.exs\"]");

        assert_eq!(
            discover_source(root, &config).unwrap(),
            vec!["lib/sample.ex".to_string(), "scripts/check.exs".to_string()]
        );
        assert!(event_relevant(root, &config, &root.join("lib/sample.ex")));
        assert!(event_relevant(
            root,
            &config,
            &root.join("scripts/check.exs")
        ));
    }

    #[test]
    fn source_normalizes_overlapping_roots_and_ignores_missing_and_pruned_roots() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write(&root.join("src/lib.rs"), b"pub fn lib() {}\n");
        write(&root.join("src/nested/mod.rs"), b"pub fn nested() {}\n");
        write(&root.join("src/generated/skip.rs"), b"generated\n");
        write(&root.join(".git/private.rs"), b"private\n");
        let config = source_config(
            "roots = [
                \"missing\",
                \"src/nested\",
                \"src\",
                \"src/lib.rs\",
                \".git\",
                \"src/generated\",
            ]\nexclude = [\"src/generated/**\"]",
        );

        assert_eq!(
            discover_source(root, &config).unwrap(),
            vec!["src/lib.rs".to_string(), "src/nested/mod.rs".to_string()]
        );
    }

    #[test]
    fn source_events_ignore_only_proven_pruned_scope() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let config = source_config("roots = [\"src\"]\nexclude = [\"src/generated/**\"]");

        assert!(event_relevant(root, &config, &root.join("src")));
        assert!(event_relevant(root, &config, &root.join("src/lib.rs")));
        assert!(!event_relevant(root, &config, &root.join("src/generated")));
        assert!(!event_relevant(
            root,
            &config,
            &root.join("src/generated/output.rs")
        ));
        assert!(!event_relevant(root, &config, &root.join("docs/note.md")));
    }

    #[test]
    fn vault_events_follow_the_vault_prune_rules() {
        let root = Path::new("/repository");
        let filter = VaultEventFilter::new(root, &Config::default());

        assert!(filter.relevant(&root.join("docs")));
        assert!(filter.relevant(&root.join("docs/note.md")));
        assert!(filter.relevant(&root.join("docs/sub")));
        assert!(!filter.relevant(&root.join("docs/node_modules/pkg/readme.md")));
        assert!(!filter.relevant(&root.join("docs/target")));
        assert!(!filter.relevant(&root.join("docs/.criv/state.json")));
        assert!(!filter.relevant(&root.join("docsite/note.md")));
        assert!(!filter.relevant(&root.join("src/lib.rs")));
        assert!(!filter.relevant(Path::new("/elsewhere/docs/note.md")));
    }

    #[test]
    fn vault_routes_lowercase_extensions_in_one_result() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write(&root.join("docs/note.md"), b"# Note\n");
        write(&root.join("docs/model.c4"), b"specification {}\n");
        write(&root.join("docs/diagram.PNG"), b"image");
        write(&root.join("docs/report.pdf"), b"document");
        write(&root.join("docs/upper.MD"), b"# Upper\n");
        write(&root.join("docs/target/skip.md"), b"# Skip\n");

        assert_eq!(
            discover_vault(root, "docs").unwrap(),
            VaultPaths {
                markdown: vec!["docs/note.md".into()],
                assets: vec!["docs/report.pdf".into()],
            }
        );
    }

    #[test]
    fn vault_includes_hidden_files_and_treats_missing_or_pruned_roots_as_empty() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write(&root.join("docs/.hidden.md"), b"# Hidden\n");
        write(&root.join("docs/.git/private.md"), b"# Private\n");
        write(&root.join("docs/node_modules/package.md"), b"# Package\n");

        assert_eq!(
            discover_vault(root, "docs").unwrap(),
            VaultPaths {
                markdown: vec!["docs/.hidden.md".into()],
                assets: vec![],
            }
        );
        assert_eq!(
            discover_vault(root, "missing").unwrap(),
            VaultPaths {
                markdown: vec![],
                assets: vec![],
            }
        );
        assert_eq!(
            discover_vault(root, "docs/target").unwrap(),
            VaultPaths {
                markdown: vec![],
                assets: vec![],
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn selected_links_fail_and_non_candidates_or_pruned_links_have_no_effect() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let outside = TempDir::new().unwrap();
        write(&outside.path().join("note.md"), b"# Outside\n");
        fs::create_dir(root.join("docs")).unwrap();
        symlink(outside.path(), root.join("docs/target")).unwrap();
        symlink(
            outside.path().join("note.md"),
            root.join("docs/non-candidate.txt"),
        )
        .unwrap();

        assert_eq!(
            discover_vault(root, "docs").unwrap(),
            VaultPaths {
                markdown: vec![],
                assets: vec![],
            }
        );

        symlink(
            outside.path().join("note.md"),
            root.join("docs/selected.md"),
        )
        .unwrap();
        assert!(
            discover_vault(root, "docs")
                .unwrap_err()
                .to_string()
                .contains("file link `docs/selected.md`")
        );
        fs::remove_file(root.join("docs/selected.md")).unwrap();

        write(
            &outside.path().join("diagram.png"),
            b"\x89PNG\r\n\x1a\npreview",
        );
        symlink(
            outside.path().join("diagram.png"),
            root.join("docs/selected.png"),
        )
        .unwrap();
        assert!(
            discover_vault(root, "docs")
                .unwrap_err()
                .to_string()
                .contains("file link `docs/selected.png`")
        );
        fs::remove_file(root.join("docs/selected.png")).unwrap();

        symlink(outside.path(), root.join("docs/active-link")).unwrap();
        assert!(
            discover_vault(root, "docs")
                .unwrap_err()
                .to_string()
                .contains("directory link `docs/active-link`")
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn non_utf8_path_identity_fails_discovery() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir(root.join("src")).unwrap();
        write(
            &root.join("src").join(OsString::from_vec(vec![b'a', 0xFF])),
            b"text\n",
        );
        let config = source_config("roots = [\"src\"]");

        assert!(
            discover_source(root, &config)
                .unwrap_err()
                .to_string()
                .contains("not valid UTF-8")
        );
    }

    #[cfg(windows)]
    #[test]
    fn vault_rejects_a_selected_windows_junction() {
        let repository = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        junction::create(outside.path(), repository.path().join("docs")).unwrap();

        assert!(
            discover_vault(repository.path(), "docs")
                .unwrap_err()
                .to_string()
                .contains("contains a link")
        );
    }
}
