//! File discovery profiles and their shared traversal rules.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use ignore::{DirEntry, Error as WalkError, ParallelVisitor, ParallelVisitorBuilder, WalkBuilder};

use crate::config::Config;
use crate::util::{GlobMatcher, is_text_file};
use crate::{CrivError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VaultPaths {
    pub(crate) markdown: Vec<String>,
    pub(crate) c4: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MarkdownPolicy<'a> {
    pub(crate) include: &'a [String],
    pub(crate) exclude: &'a [String],
    pub(crate) respect_gitignore: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
enum SelectionKind {
    Source,
    VaultMarkdown,
    VaultC4,
    Markdown,
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
    Markdown {
        include: Option<GlobMatcher>,
        exclude: GlobMatcher,
        prune: Option<GlobMatcher>,
    },
}

impl Profile {
    fn prunes_directory(&self, relative: &str, name: &str) -> bool {
        match self {
            Self::Source { prune, .. } => {
                matches!(name, ".git" | ".criv")
                    || prune.as_ref().is_some_and(|set| set.is_match(relative))
            }
            Self::Vault => matches!(name, ".git" | ".criv" | "target" | "node_modules"),
            Self::Markdown { prune, .. } => {
                prune.as_ref().is_some_and(|set| set.is_match(relative))
            }
        }
    }

    fn candidate_kind(&self, path: &Path) -> Option<SelectionKind> {
        match self {
            Self::Source { .. } => Some(SelectionKind::Source),
            Self::Vault => match path.extension().and_then(|value| value.to_str()) {
                Some("md") => Some(SelectionKind::VaultMarkdown),
                Some("c4") => Some(SelectionKind::VaultC4),
                _ => None,
            },
            Self::Markdown { .. } => path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| {
                    value.eq_ignore_ascii_case("md") || value.eq_ignore_ascii_case("markdown")
                })
                .then_some(SelectionKind::Markdown),
        }
    }

    fn selects(&self, relative: &str) -> bool {
        match self {
            Self::Source { exclude, .. } => !exclude.is_match(relative),
            Self::Vault => true,
            Self::Markdown {
                include, exclude, ..
            } => {
                include
                    .as_ref()
                    .is_none_or(|matcher| matcher.is_match(relative))
                    && !exclude.is_match(relative)
            }
        }
    }
}

pub(crate) fn discover_source(root: &Path, config: &Config) -> Result<Vec<String>> {
    if !config.source_index {
        return Ok(Vec::new());
    }

    let exclude = GlobMatcher::new(&config.source_exclude)?;
    let profile = Arc::new(Profile::Source {
        prune: subtree_prune_matcher(&config.source_exclude)?,
        exclude,
    });
    let plan = SourcePlan::new(root, &config.source_roots, profile.as_ref())?;
    let mut collected = if plan.directories.is_empty() {
        Collected::default()
    } else {
        let mut builder = WalkBuilder::empty();
        for directory in &plan.directories {
            builder.add(root.join(directory));
        }
        builder.standard_filters(false).follow_links(false);
        walk(root, &mut builder, profile.clone())?
    };

    for file in plan.files {
        collect_explicit_source(root, &file, profile.as_ref(), &mut collected);
    }
    finish(collected).map(|selections| {
        selections
            .into_iter()
            .map(|selection| selection.path)
            .collect()
    })
}

pub(crate) fn source_event_relevant(root: &Path, config: &Config, event: &Path) -> bool {
    if !config.source_index {
        return false;
    }
    let Ok(relative) = event.strip_prefix(root) else {
        return false;
    };
    let Ok(relative) = relative_utf8(root, &root.join(relative)) else {
        return true;
    };
    if relative
        .split('/')
        .any(|component| matches!(component, ".git" | ".criv"))
    {
        return false;
    }
    let roots = match config
        .source_roots
        .iter()
        .map(|value| normalize_relative("source.roots", value))
        .collect::<Result<Vec<_>>>()
    {
        Ok(roots) => roots,
        Err(_) => return true,
    };
    let intersects_root = roots.iter().any(|source_root| {
        path_contains(source_root, &relative) || path_contains(&relative, source_root)
    });
    if !intersects_root {
        return false;
    }
    let exclude = match GlobMatcher::new(&config.source_exclude) {
        Ok(exclude) => exclude,
        Err(_) => return true,
    };
    let prune = match subtree_prune_matcher(&config.source_exclude) {
        Ok(prune) => prune,
        Err(_) => return true,
    };
    let profile = Profile::Source { exclude, prune };
    if profile_prunes_path(&profile, &relative) {
        return false;
    }
    profile.selects(&relative)
}

pub(crate) fn discover_vault(root: &Path, docs_dir: &str) -> Result<VaultPaths> {
    let docs_relative = normalize_relative("vault.docs", docs_dir)?;
    let profile = Arc::new(Profile::Vault);
    if profile_prunes_path(profile.as_ref(), &docs_relative) {
        return Ok(VaultPaths {
            markdown: Vec::new(),
            c4: Vec::new(),
        });
    }
    let Some(kind) = validate_root(root, &docs_relative)? else {
        return Ok(VaultPaths {
            markdown: Vec::new(),
            c4: Vec::new(),
        });
    };
    if kind != RootKind::Directory {
        return Err(CrivError::new(format!(
            "vault walk root `{docs_relative}` must be a real directory"
        )));
    }

    let mut builder = WalkBuilder::new(root.join(&docs_relative));
    builder.standard_filters(false).follow_links(false);
    let selections = finish(walk(root, &mut builder, profile)?)?;
    let mut markdown = Vec::new();
    let mut c4 = Vec::new();
    for selection in selections {
        match selection.kind {
            SelectionKind::VaultMarkdown => markdown.push(selection.path),
            SelectionKind::VaultC4 => c4.push(selection.path),
            SelectionKind::Source | SelectionKind::Markdown => {}
        }
    }
    Ok(VaultPaths { markdown, c4 })
}

pub(crate) fn discover_markdown(root: &Path, policy: MarkdownPolicy<'_>) -> Result<Vec<String>> {
    let include = (!policy.include.is_empty())
        .then(|| GlobMatcher::new(policy.include))
        .transpose()?;
    let exclude = GlobMatcher::new(policy.exclude)?;
    let profile = Arc::new(Profile::Markdown {
        include,
        prune: subtree_prune_matcher(policy.exclude)?,
        exclude,
    });
    let mut builder = markdown_builder(root, policy);
    finish(walk(root, &mut builder, profile)?)
        .map(|selections| selections.into_iter().map(|item| item.path).collect())
}

pub(crate) fn select_markdown(
    root: &Path,
    policy: MarkdownPolicy<'_>,
    candidates: &BTreeSet<String>,
) -> Result<Vec<String>> {
    let include = (!policy.include.is_empty())
        .then(|| GlobMatcher::new(policy.include))
        .transpose()?;
    let exclude = GlobMatcher::new(policy.exclude)?;
    let profile = Profile::Markdown {
        include,
        prune: subtree_prune_matcher(policy.exclude)?,
        exclude,
    };
    let builder = markdown_builder(root, policy);
    let mut matchers = builder.build_matchers();
    let mut matcher = matchers
        .pop()
        .ok_or_else(|| CrivError::new("Markdown ignore matcher has no repository root"))?;
    let mut selected = Vec::new();
    let mut errors = Vec::new();
    for candidate in candidates {
        let relative = normalize_relative("changed path", candidate)?;
        if policy.include.is_empty() && has_hidden_component(&relative) {
            continue;
        }
        let path = root.join(&relative);
        if profile.candidate_kind(&path) != Some(SelectionKind::Markdown)
            || !profile.selects(&relative)
        {
            continue;
        }
        let (matched, error) = matcher.matched_with_errors(Path::new(&relative), false);
        if let Some(error) = error {
            errors.push(normalize_walk_error(root, error));
        }
        if matched.is_ignore() {
            continue;
        }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                errors.push(path_error(root, &path, error));
                continue;
            }
        };
        if metadata.file_type().is_symlink() || is_junction(&path) {
            errors.push(format!("refusing to discover file link `{relative}`"));
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        match fs::File::open(&path) {
            Ok(_) => selected.push(relative),
            Err(error) => errors.push(format!(
                "failed to read selected file `{relative}`: {error}"
            )),
        }
    }
    errors.sort();
    errors.dedup();
    if !errors.is_empty() {
        return Err(CrivError::new(format!(
            "file discovery failed:\n{}",
            errors
                .into_iter()
                .map(|error| format!("- {error}"))
                .collect::<Vec<_>>()
                .join("\n")
        )));
    }
    selected.sort();
    selected.dedup();
    Ok(selected)
}

fn has_hidden_component(relative: &str) -> bool {
    relative
        .split('/')
        .any(|component| component.starts_with('.') && component != ".")
}

fn markdown_builder(root: &Path, policy: MarkdownPolicy<'_>) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(policy.include.is_empty())
        .parents(policy.respect_gitignore)
        .ignore(policy.respect_gitignore)
        .git_ignore(policy.respect_gitignore)
        .git_global(policy.respect_gitignore)
        .git_exclude(policy.respect_gitignore)
        .require_git(true)
        .follow_links(false)
        .current_dir(root);
    builder
}

fn walk(root: &Path, builder: &mut WalkBuilder, profile: Arc<Profile>) -> Result<Collected> {
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
    let collected = std::mem::take(
        &mut *shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    Ok(collected)
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
                    .push(normalize_walk_error(&self.root, error));
                return ignore::WalkState::Continue;
            }
        };
        if let Some(error) = entry.error() {
            self.local
                .errors
                .push(normalize_walk_error(&self.root, error.clone()));
        }
        if entry.depth() == 0 && entry.path() == self.root.as_path() {
            return ignore::WalkState::Continue;
        }
        self.collect_entry(entry.path(), entry.path_is_symlink());
        ignore::WalkState::Continue
    }
}

impl Collector {
    fn collect_entry(&mut self, path: &Path, reported_symlink: bool) {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => {
                self.local.errors.push(path_error(&self.root, path, error));
                return;
            }
        };
        let is_link = reported_symlink || metadata.file_type().is_symlink() || is_junction(path);
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
            self.local.errors.push(format!(
                "refusing to discover directory link `{}`",
                relative
            ));
            return;
        }
        if metadata.is_dir() {
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
        if !metadata.is_file() {
            return;
        }
        let accessible = match kind {
            SelectionKind::Source => is_text_file(path),
            SelectionKind::VaultMarkdown | SelectionKind::VaultC4 | SelectionKind::Markdown => {
                fs::File::open(path).map(|_| true).map_err(CrivError::from)
            }
        };
        match accessible {
            Ok(true) => self.local.selections.push(Selection {
                kind,
                path: relative,
            }),
            Ok(false) => {}
            Err(error) => self.local.errors.push(format!(
                "failed to read selected file `{relative}`: {error}"
            )),
        }
    }
}

impl Drop for Collector {
    fn drop(&mut self) {
        let mut shared = self
            .shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        shared.selections.append(&mut self.local.selections);
        shared.errors.append(&mut self.local.errors);
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
    match is_text_file(&path) {
        Ok(true) => collected.selections.push(Selection {
            kind: SelectionKind::Source,
            path: relative.to_string(),
        }),
        Ok(false) => {}
        Err(error) => collected.errors.push(format!(
            "failed to read selected file `{relative}`: {error}"
        )),
    }
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
        let normalized = roots
            .iter()
            .map(|value| normalize_relative("source.roots", value))
            .collect::<Result<BTreeSet<_>>>()?;
        let mut directories = Vec::new();
        let mut files = Vec::new();
        for relative in normalized {
            if profile_prunes_path(profile, &relative) {
                continue;
            }
            match validate_root(root, &relative)? {
                Some(RootKind::Directory) => directories.push(relative),
                Some(RootKind::File) => files.push(relative),
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
            Ok(metadata) if index + 1 < components.len() && !metadata.is_dir() => {
                return Err(CrivError::new(format!(
                    "discovery root `{relative}` has a non-directory component `{}`",
                    display_relative(root, &current)
                )));
            }
            Ok(metadata) if index + 1 == components.len() => {
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

fn normalize_relative(field: &str, value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(CrivError::new(format!("{field} must not be empty")));
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return Err(CrivError::new(format!(
            "{field} `{value}` must be relative to the criv vault root"
        )));
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str().ok_or_else(|| {
                    CrivError::new(format!("{field} `{value}` is not valid UTF-8"))
                })?;
                parts.push(part.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err(CrivError::new(format!(
                        "{field} `{value}` escapes the criv vault root"
                    )));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(CrivError::new(format!(
                    "{field} `{value}` must be relative to the criv vault root"
                )));
            }
        }
    }
    Ok(if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    })
}

fn relative_utf8(root: &Path, path: &Path) -> Result<String> {
    let relative = path.strip_prefix(root).map_err(|_| {
        CrivError::new(format!(
            "discovered path {} is outside repository root {}",
            path.display(),
            root.display()
        ))
    })?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(
                part.to_str()
                    .ok_or_else(|| {
                        CrivError::new(format!(
                            "discovered path {} is not valid UTF-8",
                            path.display()
                        ))
                    })?
                    .to_string(),
            ),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(CrivError::new(format!(
                    "discovered path {} has an invalid repository identity",
                    path.display()
                )));
            }
        }
    }
    Ok(parts.join("/"))
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

fn normalize_walk_error(root: &Path, error: WalkError) -> String {
    error
        .to_string()
        .replace(&root.to_string_lossy().to_string(), ".")
}

fn path_error(root: &Path, path: &Path, error: std::io::Error) -> String {
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
fn is_junction(_path: &Path) -> bool {
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

    #[test]
    fn source_uses_roots_excludes_hidden_and_binary_rules() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write(&root.join("src/lib.rs"), b"pub fn lib() {}\n");
        write(&root.join("src/.hidden.rs"), b"pub fn hidden() {}\n");
        write(&root.join("src/ignored.rs"), b"pub fn ignored() {}\n");
        write(&root.join("src/.gitignore"), b"ignored.rs\n");
        write(&root.join("src/excluded.rs"), b"pub fn excluded() {}\n");
        write(&root.join("src/binary.rs"), b"\0binary");
        write(&root.join(".git/private.rs"), b"private\n");
        let config = Config {
            source_roots: vec!["src".into(), "src/./nested/..".into()],
            source_exclude: vec!["src/excluded.rs".into()],
            ..Config::default()
        };

        assert_eq!(
            discover_source(root, &config).unwrap(),
            vec![
                "src/.gitignore".to_string(),
                "src/.hidden.rs".to_string(),
                "src/ignored.rs".to_string(),
                "src/lib.rs".to_string(),
            ]
        );
    }

    #[test]
    fn source_normalizes_overlapping_roots_and_ignores_missing_and_pruned_roots() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write(&root.join("src/lib.rs"), b"pub fn lib() {}\n");
        write(&root.join("src/nested/mod.rs"), b"pub fn nested() {}\n");
        write(&root.join("src/generated/skip.rs"), b"generated\n");
        write(&root.join(".git/private.rs"), b"private\n");
        let config = Config {
            source_roots: vec![
                "missing".into(),
                "src/nested".into(),
                "src".into(),
                "src/lib.rs".into(),
                ".git".into(),
                "src/generated".into(),
            ],
            source_exclude: vec!["src/generated/**".into()],
            ..Config::default()
        };

        assert_eq!(
            discover_source(root, &config).unwrap(),
            vec!["src/lib.rs".to_string(), "src/nested/mod.rs".to_string()]
        );
    }

    #[test]
    fn source_events_ignore_only_proven_pruned_scope() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let config = Config {
            source_roots: vec!["src".into()],
            source_exclude: vec!["src/generated/**".into()],
            ..Config::default()
        };

        assert!(source_event_relevant(root, &config, &root.join("src")));
        assert!(source_event_relevant(
            root,
            &config,
            &root.join("src/lib.rs")
        ));
        assert!(!source_event_relevant(
            root,
            &config,
            &root.join("src/generated")
        ));
        assert!(!source_event_relevant(
            root,
            &config,
            &root.join("src/generated/output.rs")
        ));
        assert!(!source_event_relevant(
            root,
            &config,
            &root.join("docs/note.md")
        ));
    }

    #[test]
    fn source_uses_one_binary_contract_for_directory_and_explicit_roots() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let cases = vec![
            ("empty", Vec::new(), true),
            ("ascii", b"plain text\n".to_vec(), true),
            ("utf8", "hello é\n".as_bytes().to_vec(), true),
            ("invalid-utf8", vec![0x80, 0x81, b'x'], true),
            ("nul", vec![b'a', 0, b'b'], false),
            ("utf8-bom", vec![0xEF, 0xBB, 0xBF, b'x'], true),
            ("utf16-le-bom", vec![0xFF, 0xFE, b'x', 0], true),
            ("utf16-be-bom", vec![0xFE, 0xFF, 0, b'x'], true),
            ("utf32-le-bom", vec![0xFF, 0xFE, 0, 0, b'x', 0, 0, 0], true),
            ("utf32-be-bom", vec![0, 0, 0xFE, 0xFF, 0, 0, 0, b'x'], true),
            ("utf16-no-bom", vec![b'x', 0, b'y', 0], false),
            ("pdf", b"%PDF-1.7".to_vec(), false),
            ("png", vec![0x89, b'P', b'N', b'G', b'x'], false),
            ("binary-pgm", b"P5\n1 1\n255\n\xff".to_vec(), true),
            ("small-protobuf", vec![0x08, 0x96, 0x01], true),
            (
                "generated-text",
                b"(()=>{const map={version:3,sources:[]};return map})()\n".to_vec(),
                true,
            ),
            (
                "nul-at-1023",
                {
                    let mut bytes = vec![b'x'; 1024];
                    bytes[1023] = 0;
                    bytes
                },
                false,
            ),
            (
                "nul-at-1024",
                {
                    let mut bytes = vec![b'x'; 1025];
                    bytes[1024] = 0;
                    bytes
                },
                true,
            ),
        ];
        let mut roots = vec!["directory".to_string()];
        let mut expected = Vec::new();
        for (name, bytes, is_text) in cases {
            let directory = format!("directory/{name}.rs");
            let explicit = format!("explicit/{name}.bin");
            write(&root.join(&directory), &bytes);
            write(&root.join(&explicit), &bytes);
            roots.push(explicit.clone());
            if is_text {
                expected.push(directory);
                expected.push(explicit);
            }
        }
        expected.sort();
        let config = Config {
            source_roots: roots,
            ..Config::default()
        };

        assert_eq!(discover_source(root, &config).unwrap(), expected);
    }

    #[test]
    fn vault_routes_lowercase_extensions_in_one_result() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write(&root.join("docs/note.md"), b"# Note\n");
        write(&root.join("docs/model.c4"), b"specification {}\n");
        write(&root.join("docs/upper.MD"), b"# Upper\n");
        write(&root.join("docs/target/skip.md"), b"# Skip\n");

        assert_eq!(
            discover_vault(root, "docs").unwrap(),
            VaultPaths {
                markdown: vec!["docs/note.md".into()],
                c4: vec!["docs/model.c4".into()],
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
                c4: vec![],
            }
        );
        assert_eq!(
            discover_vault(root, "missing").unwrap(),
            VaultPaths {
                markdown: vec![],
                c4: vec![],
            }
        );
        assert_eq!(
            discover_vault(root, "docs/target").unwrap(),
            VaultPaths {
                markdown: vec![],
                c4: vec![],
            }
        );
    }

    #[test]
    fn markdown_validates_patterns_and_respect_gitignore() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write(&root.join("README.MARKDOWN"), b"# Readme\n");
        write(&root.join("docs/keep.md"), b"# Keep\n");
        write(&root.join("docs/skip.md"), b"# Skip\n");
        write(&root.join(".gitignore"), b"docs/skip.md\n");
        fs::create_dir(root.join(".git")).unwrap();

        let policy = MarkdownPolicy {
            include: &[],
            exclude: &[],
            respect_gitignore: true,
        };
        assert_eq!(
            discover_markdown(root, policy).unwrap(),
            vec!["README.MARKDOWN".to_string(), "docs/keep.md".to_string()]
        );

        let invalid = ["[".to_string()];
        assert!(
            discover_markdown(
                root,
                MarkdownPolicy {
                    include: &invalid,
                    exclude: &[],
                    respect_gitignore: false,
                }
            )
            .is_err()
        );
    }

    #[test]
    fn markdown_uses_non_git_ignore_rules_only_when_configured() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write(&root.join("git-rule.md"), b"# Git rule\n");
        write(&root.join("ignore-rule.md"), b"# Ignore rule\n");
        write(&root.join(".gitignore"), b"git-rule.md\n");
        write(&root.join(".ignore"), b"ignore-rule.md\n");

        assert_eq!(
            discover_markdown(
                root,
                MarkdownPolicy {
                    include: &[],
                    exclude: &[],
                    respect_gitignore: true,
                }
            )
            .unwrap(),
            vec!["git-rule.md".to_string()]
        );
        assert_eq!(
            discover_markdown(
                root,
                MarkdownPolicy {
                    include: &[],
                    exclude: &[],
                    respect_gitignore: false,
                }
            )
            .unwrap(),
            vec!["git-rule.md".to_string(), "ignore-rule.md".to_string()]
        );
    }

    #[test]
    fn markdown_explicit_includes_select_hidden_files_and_validate_excludes() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write(&root.join(".hidden.md"), b"# Hidden\n");
        write(&root.join("visible.md"), b"# Visible\n");
        let include = vec![".hidden.md".to_string()];

        assert_eq!(
            discover_markdown(
                root,
                MarkdownPolicy {
                    include: &include,
                    exclude: &[],
                    respect_gitignore: false,
                }
            )
            .unwrap(),
            vec![".hidden.md".to_string()]
        );

        let invalid = vec!["[".to_string()];
        assert!(
            discover_markdown(
                root,
                MarkdownPolicy {
                    include: &[],
                    exclude: &invalid,
                    respect_gitignore: false,
                }
            )
            .is_err()
        );
    }

    #[test]
    fn changed_markdown_uses_the_same_hidden_file_policy() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write(&root.join(".hidden.md"), b"# Hidden\n");
        write(&root.join("visible.md"), b"# Visible\n");
        let candidates = BTreeSet::from([".hidden.md".to_string(), "visible.md".to_string()]);

        assert_eq!(
            select_markdown(
                root,
                MarkdownPolicy {
                    include: &[],
                    exclude: &[],
                    respect_gitignore: false,
                },
                &candidates,
            )
            .unwrap(),
            vec!["visible.md".to_string()]
        );

        let include = vec![".hidden.md".to_string()];
        assert_eq!(
            select_markdown(
                root,
                MarkdownPolicy {
                    include: &include,
                    exclude: &[],
                    respect_gitignore: false,
                },
                &candidates,
            )
            .unwrap(),
            vec![".hidden.md".to_string()]
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
                c4: vec![],
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

        symlink(outside.path(), root.join("docs/active-link")).unwrap();
        assert!(
            discover_vault(root, "docs")
                .unwrap_err()
                .to_string()
                .contains("directory link `docs/active-link`")
        );
    }

    #[cfg(unix)]
    #[test]
    fn selected_read_errors_fail_and_pruned_read_errors_have_no_effect() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write(&root.join("src/selected.rs"), b"pub fn selected() {}\n");
        write(&root.join("src/pruned/hidden.rs"), b"pub fn hidden() {}\n");
        let selected = root.join("src/selected.rs");
        let pruned = root.join("src/pruned");
        fs::set_permissions(&selected, fs::Permissions::from_mode(0o000)).unwrap();
        fs::set_permissions(&pruned, fs::Permissions::from_mode(0o000)).unwrap();
        let config = Config {
            source_roots: vec!["src".into()],
            source_exclude: vec!["src/pruned/**".into()],
            ..Config::default()
        };

        let can_read_without_permission = fs::File::open(&selected).is_ok();
        let result = discover_source(root, &config);
        fs::set_permissions(&selected, fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(&pruned, fs::Permissions::from_mode(0o700)).unwrap();
        if can_read_without_permission {
            return;
        }

        let error = result.unwrap_err().to_string();
        assert!(error.contains("failed to read selected file `src/selected.rs`"));
        assert!(!error.contains("src/pruned"));
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
        let config = Config {
            source_roots: vec!["src".into()],
            ..Config::default()
        };

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
