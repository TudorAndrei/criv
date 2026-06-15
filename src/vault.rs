use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::Result;
use crate::config::Config;
use crate::source_graph::SourceGraph;
use crate::source_index::{FffSourceIndex, SourceIndex};
use crate::util::{
    GlobMatcher, find_wiki_links_with_lines, glob_matches, is_text_file, kebab,
    markdown_headings as parse_markdown_headings, read_to_string, strip_prefix, walk_files,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoteKind {
    Doc,
    Decision,
    Unknown,
}

#[derive(Debug, Clone)]
pub(crate) struct PatternRef {
    pub(crate) id: String,
    pub(crate) line: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct WikiLink {
    pub(crate) raw: String,
    pub(crate) target: String,
    pub(crate) line: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct Heading {
    pub(crate) level: usize,
    pub(crate) text: String,
    pub(crate) line: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct Note {
    pub(crate) path: PathBuf,
    pub(crate) rel_path: String,
    pub(crate) id: Option<String>,
    pub(crate) kind: NoteKind,
    pub(crate) title: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) body: String,
    pub(crate) headings: Vec<Heading>,
    pub(crate) targets_symbols: Vec<String>,
    pub(crate) targets_scope: Vec<String>,
    pub(crate) target_pattern_refs: Vec<PatternRef>,
    pub(crate) target_pattern_ids: Vec<String>,
    pub(crate) policy_pattern_ids: Vec<String>,
    pub(crate) governs: Vec<String>,
    pub(crate) supersedes: Vec<String>,
    pub(crate) superseded_by: Vec<String>,
    pub(crate) wiki_links: Vec<WikiLink>,
    pub(crate) frontmatter_error: Option<String>,
}

impl Note {
    pub(crate) fn display_id(&self) -> &str {
        self.id.as_deref().unwrap_or(&self.rel_path)
    }

    pub(crate) fn filename_stem(&self) -> Option<String> {
        self.path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
    }
}

#[derive(Debug)]
pub(crate) struct Vault {
    pub(crate) config: Config,
    pub(crate) notes: Vec<Note>,
    note_ids: BTreeMap<String, usize>,
    filenames: BTreeMap<String, usize>,
    titles: BTreeMap<String, usize>,
    source_files: Vec<String>,
    source_index: Box<dyn SourceIndex>,
    source_graph: SourceGraph,
    patterns: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResolvedLink {
    Source { path: String, ambiguous: bool },
    Pattern { id: String },
    Note { id: String },
    Broken,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SourceTargetResolution {
    Resolved { path: String, ambiguous: bool },
    MissingFile,
    MissingFragment { path: String },
}

impl Vault {
    pub(crate) fn load(root: &Path) -> Result<Self> {
        Self::load_incremental(root, None)
    }

    pub(crate) fn load_incremental(
        root: &Path,
        previous_graph: Option<&SourceGraph>,
    ) -> Result<Self> {
        let config = Config::load(root)?;
        let docs_path = config.docs_path(root);
        let notes = walk_files(&docs_path, Some("md"))?
            .into_iter()
            .map(|path| parse_note(root, &docs_path, &path))
            .collect::<Result<Vec<_>>>()?;

        let mut note_ids = BTreeMap::new();
        let mut filenames = BTreeMap::new();
        let mut titles = BTreeMap::new();
        for (index, note) in notes.iter().enumerate() {
            if let Some(id) = &note.id {
                note_ids.entry(id.to_lowercase()).or_insert(index);
            }
            if let Some(stem) = note.filename_stem() {
                filenames.entry(stem.to_lowercase()).or_insert(index);
            }
            if let Some(title) = &note.title {
                titles.entry(title.to_lowercase()).or_insert(index);
            }
        }

        let mut patterns = config.patterns.clone();
        for note in &notes {
            if let Some(id) = &note.id {
                for pattern in &note.policy_pattern_ids {
                    patterns.insert(format!("{id}/{pattern}"));
                }
            }
        }

        let (source_files, source_index, source_graph): (
            Vec<String>,
            Box<dyn SourceIndex>,
            SourceGraph,
        ) = if config.source_index {
            let source_files = collect_source_files(root, &config)?;
            let source_index = Box::new(FffSourceIndex::new(
                root,
                &config.source_roots,
                &config.source_exclude,
                false,
            )?);
            let source_graph = SourceGraph::build_incremental(root, &source_files, previous_graph)?;
            (source_files, source_index, source_graph)
        } else {
            (
                Vec::new(),
                Box::new(EmptySourceIndex),
                SourceGraph::default(),
            )
        };

        Ok(Self {
            config,
            notes,
            note_ids,
            filenames,
            titles,
            source_files,
            source_index,
            source_graph,
            patterns,
        })
    }

    pub(crate) fn resolve_note(&self, target: &str) -> Option<&Note> {
        let key = target.to_lowercase();
        self.note_ids
            .get(&key)
            .or_else(|| self.filenames.get(&key))
            .or_else(|| self.titles.get(&key))
            .and_then(|index| self.notes.get(*index))
    }

    pub(crate) fn is_file_backed_note_target(&self, target: &str) -> bool {
        let target = target.split('|').next().unwrap_or(target).trim();
        let target_without_heading = target.split('#').next().unwrap_or(target).trim();
        let key = target_without_heading.to_lowercase();
        let Some(note) = self.resolve_note(target_without_heading) else {
            return false;
        };

        if note
            .filename_stem()
            .is_some_and(|stem| stem.eq_ignore_ascii_case(&key))
        {
            return true;
        }

        let rel_path = note.rel_path.to_lowercase();
        key == rel_path || rel_path.strip_suffix(".md") == Some(key.as_str())
    }

    pub(crate) fn portable_note_target(&self, target: &str) -> Option<String> {
        let target = target.split('|').next().unwrap_or(target).trim();
        let (target_without_heading, heading) = target
            .split_once('#')
            .map(|(base, heading)| (base, Some(heading)))
            .unwrap_or((target, None));
        let note = self.resolve_note(target_without_heading.trim())?;
        let mut portable = note.filename_stem()?;
        if let Some(heading) = heading
            && !heading.is_empty()
        {
            portable.push('#');
            portable.push_str(heading);
        }
        if portable == target {
            Some(portable)
        } else {
            Some(format!("{portable}|{target}"))
        }
    }

    pub(crate) fn resolve_link(&self, target: &str) -> ResolvedLink {
        let target = target.split('|').next().unwrap_or(target).trim();

        match self.resolve_source_target(target) {
            SourceTargetResolution::Resolved { path, ambiguous } => {
                return ResolvedLink::Source { path, ambiguous };
            }
            SourceTargetResolution::MissingFragment { .. } => return ResolvedLink::Broken,
            SourceTargetResolution::MissingFile => {}
        }

        if let Some(pattern_id) = pattern_link_id(target) {
            if self.patterns.contains(pattern_id) {
                return ResolvedLink::Pattern {
                    id: pattern_id.to_string(),
                };
            }
            return ResolvedLink::Broken;
        }

        let note_target = target.split('#').next().unwrap_or(target);
        if let Some(note) = self.resolve_note(note_target) {
            if let Some((_, heading)) = target.split_once('#')
                && !heading.is_empty()
                && !note_has_heading(note, heading)
            {
                return ResolvedLink::Broken;
            }
            return ResolvedLink::Note {
                id: note.display_id().to_string(),
            };
        }

        ResolvedLink::Broken
    }

    pub(crate) fn resolve_source_path(&self, path: &str) -> Option<(String, bool)> {
        self.source_index.resolve_partial_path(path)
    }

    pub(crate) fn resolve_source_target(&self, target: &str) -> SourceTargetResolution {
        let target = target.split('|').next().unwrap_or(target).trim();
        let Some((path, ambiguous)) = self.resolve_source_path(source_fragment_path(target)) else {
            return SourceTargetResolution::MissingFile;
        };
        let Some(fragment) = source_fragment_name(target) else {
            return SourceTargetResolution::Resolved { path, ambiguous };
        };

        if self
            .source_graph
            .resolve_symbol(&format!("{path}#{fragment}"))
            .is_some()
        {
            SourceTargetResolution::Resolved { path, ambiguous }
        } else {
            SourceTargetResolution::MissingFragment { path }
        }
    }

    pub(crate) fn source_glob_has_match(&self, pattern: &str) -> bool {
        self.source_files
            .iter()
            .any(|source_file| glob_matches(pattern, source_file))
    }

    pub(crate) fn source_files_matching_glob(&self, pattern: &str) -> Vec<String> {
        self.source_files
            .iter()
            .filter(|source_file| glob_matches(pattern, source_file))
            .cloned()
            .collect()
    }

    pub(crate) fn source_files(&self) -> &[String] {
        &self.source_files
    }

    pub(crate) fn source_graph(&self) -> &SourceGraph {
        &self.source_graph
    }

    pub(crate) fn source_index(&self) -> &dyn SourceIndex {
        self.source_index.as_ref()
    }

    pub(crate) fn patterns(&self) -> &BTreeSet<String> {
        &self.patterns
    }

    pub(crate) fn effective_governs(&self, note: &Note) -> Vec<String> {
        if note.kind == NoteKind::Decision && note.governs.is_empty() {
            vec!["**".into()]
        } else {
            note.governs.clone()
        }
    }

    #[cfg(test)]
    pub(crate) fn from_parts_for_test(notes: Vec<Note>) -> Self {
        let mut note_ids = BTreeMap::new();
        let mut filenames = BTreeMap::new();
        let mut titles = BTreeMap::new();
        for (index, note) in notes.iter().enumerate() {
            if let Some(id) = &note.id {
                note_ids.entry(id.to_lowercase()).or_insert(index);
            }
            if let Some(stem) = note.filename_stem() {
                filenames.entry(stem.to_lowercase()).or_insert(index);
            }
            if let Some(title) = &note.title {
                titles.entry(title.to_lowercase()).or_insert(index);
            }
        }

        Self {
            config: Config::default(),
            notes,
            note_ids,
            filenames,
            titles,
            source_files: Vec::new(),
            source_index: Box::new(EmptySourceIndex),
            source_graph: SourceGraph::default(),
            patterns: BTreeSet::new(),
        }
    }
}

#[derive(Debug)]
struct EmptySourceIndex;

impl SourceIndex for EmptySourceIndex {
    fn fuzzy_files(
        &self,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<crate::source_index::FileHit>> {
        Ok(Vec::new())
    }

    fn grep(
        &self,
        _query: &str,
        _mode: crate::source_index::SourceGrepMode,
        _paths: &[String],
    ) -> Result<Vec<crate::source_index::GrepHit>> {
        Ok(Vec::new())
    }

    fn resolve_partial_path(&self, _path: &str) -> Option<(String, bool)> {
        None
    }

    fn entries(&self) -> Result<Vec<crate::source_index::IndexedSource>> {
        Ok(Vec::new())
    }

    fn source_fingerprint(&self) -> Result<String> {
        Ok(String::new())
    }
}

fn parse_note(root: &Path, docs_path: &Path, path: &Path) -> Result<Note> {
    let contents = read_to_string(path)?;
    let rel_path = strip_prefix(path, root);
    let (frontmatter, body) = split_frontmatter(&contents);
    let doc_rel_path = strip_prefix(path, docs_path);
    let mut note = match parse_frontmatter(frontmatter, path.to_path_buf(), doc_rel_path, body) {
        Ok(note) => note,
        Err(err) => Note {
            path: path.to_path_buf(),
            rel_path: strip_prefix(path, docs_path),
            id: None,
            kind: NoteKind::Unknown,
            title: None,
            status: None,
            body: contents.clone(),
            headings: Vec::new(),
            targets_symbols: Vec::new(),
            targets_scope: Vec::new(),
            target_pattern_refs: Vec::new(),
            target_pattern_ids: Vec::new(),
            policy_pattern_ids: Vec::new(),
            governs: Vec::new(),
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            wiki_links: Vec::new(),
            frontmatter_error: Some(err),
        },
    };
    note.wiki_links = find_wiki_links_with_lines(&note.body)
        .into_iter()
        .map(|(line, raw)| WikiLink {
            target: raw.split('|').next().unwrap_or(&raw).trim().to_string(),
            raw,
            line,
        })
        .collect();
    note.headings = parse_markdown_headings(&note.body)
        .into_iter()
        .map(|(level, text, line)| Heading { level, text, line })
        .collect();
    note.rel_path = rel_path;
    Ok(note)
}

fn split_frontmatter(contents: &str) -> (&str, String) {
    let Some(rest) = contents.strip_prefix("---\n") else {
        return ("", contents.to_string());
    };
    let Some(end) = rest.find("\n---") else {
        return ("", contents.to_string());
    };
    let frontmatter = &rest[..end];
    let body_start = end + "\n---".len();
    let body = rest[body_start..].trim_start_matches('\n').to_string();
    (frontmatter, body)
}

fn parse_frontmatter(
    frontmatter: &str,
    path: PathBuf,
    rel_path: String,
    body: String,
) -> std::result::Result<Note, String> {
    let raw = if frontmatter.trim().is_empty() {
        RawFrontmatter::default()
    } else {
        serde_norway::from_str::<RawFrontmatter>(frontmatter)
            .map_err(|err| format!("failed to parse YAML frontmatter: {err}"))?
    };

    let kind = match raw.kind.as_deref() {
        Some("doc") => NoteKind::Doc,
        Some("decision") => NoteKind::Decision,
        _ => NoteKind::Unknown,
    };

    let mut targets_symbols = Vec::new();
    let mut targets_scope = Vec::new();
    let mut target_pattern_refs = Vec::new();
    let mut target_pattern_ids = Vec::new();
    if let Some(targets) = raw.targets {
        match targets {
            RawTargets::Symbols(symbols) => targets_symbols = symbols,
            RawTargets::Object(targets) => {
                targets_symbols = targets.symbols;
                targets_scope = targets.scope;
                for pattern in targets.patterns {
                    if let Some(id) = pattern.reference {
                        target_pattern_refs.push(PatternRef {
                            line: frontmatter_line(frontmatter, &id),
                            id,
                        });
                    }
                    if let Some(id) = pattern.id {
                        target_pattern_ids.push(id);
                    }
                }
            }
        }
    }

    let policy_pattern_ids = raw
        .policy
        .map(|policy| {
            policy
                .patterns
                .into_iter()
                .filter_map(|pattern| pattern.id)
                .collect()
        })
        .unwrap_or_default();

    Ok(Note {
        path,
        rel_path,
        id: raw.id,
        kind,
        title: raw.title,
        status: raw.status,
        body,
        headings: Vec::new(),
        targets_symbols,
        targets_scope,
        target_pattern_refs,
        target_pattern_ids,
        policy_pattern_ids,
        governs: raw.governs,
        supersedes: raw.supersedes,
        superseded_by: raw.superseded_by,
        wiki_links: Vec::new(),
        frontmatter_error: None,
    })
}

fn pattern_link_id(target: &str) -> Option<&str> {
    if let Some(id) = target.strip_prefix("match:") {
        return Some(id);
    }
    target
        .split_once("#match:")
        .map(|(_, pattern_id)| pattern_id)
}

fn collect_source_files(root: &Path, config: &Config) -> Result<Vec<String>> {
    let mut files = Vec::new();
    let excludes = GlobMatcher::new(&config.source_exclude)?;
    for source_root in config.source_root_paths(root) {
        for entry in ignore::WalkBuilder::new(&source_root)
            .hidden(false)
            .git_ignore(true)
            .git_exclude(true)
            .parents(true)
            .build()
        {
            let entry = entry.map_err(|err| crate::CrivError::new(err.to_string()))?;
            if !entry
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
            {
                continue;
            }
            let path = entry.path();
            let rel = strip_prefix(path, root);
            if excludes.is_match(&rel) || !is_text_file(path)? {
                continue;
            }
            files.push(rel);
        }
    }
    files.sort();
    Ok(files)
}

pub(crate) fn source_fragment_path(value: &str) -> &str {
    value.split('#').next().unwrap_or(value)
}

pub(crate) fn source_fragment_name(value: &str) -> Option<&str> {
    let fragment = value.split('|').next().unwrap_or(value).split_once('#')?.1;
    (!fragment.is_empty() && !is_line_fragment(fragment)).then_some(fragment)
}

fn is_line_fragment(fragment: &str) -> bool {
    let parse_line = |value: &str| {
        value
            .strip_prefix('L')
            .and_then(|line| line.parse::<usize>().ok())
            .is_some_and(|line| line > 0)
    };
    match fragment.split_once('-') {
        Some((start, end)) => parse_line(start) && parse_line(end),
        None => parse_line(fragment),
    }
}

fn note_has_heading(note: &Note, heading: &str) -> bool {
    let normalized = heading.trim();
    note.headings.iter().any(|candidate| {
        candidate.text.eq_ignore_ascii_case(normalized)
            || kebab(&candidate.text) == kebab(normalized)
    })
}

fn frontmatter_line(frontmatter: &str, needle: &str) -> usize {
    frontmatter
        .lines()
        .position(|line| line.contains(needle))
        .map(|index| index + 2)
        .unwrap_or(2)
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawFrontmatter {
    id: Option<String>,
    kind: Option<String>,
    title: Option<String>,
    status: Option<String>,
    targets: Option<RawTargets>,
    governs: Vec<String>,
    supersedes: Vec<String>,
    superseded_by: Vec<String>,
    policy: Option<RawPolicy>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawTargets {
    Symbols(Vec<String>),
    Object(RawTargetsObject),
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawTargetsObject {
    scope: Vec<String>,
    symbols: Vec<String>,
    patterns: Vec<RawPatternRef>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawPatternRef {
    id: Option<String>,
    #[serde(rename = "ref")]
    reference: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawPolicy {
    patterns: Vec<RawPatternRef>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_core_frontmatter_fields() {
        let note = parse_frontmatter(
            r#"id: ADR-0007
kind: decision
title: No block_on
status: accepted
supersedes: [ADR-0001]
governs:
  - src/**
policy:
  patterns:
    - id: no-block-on
targets:
  symbols:
    - src/lib.rs#run
  patterns:
    - { ref: ADR-0007/no-block-on }
"#,
            PathBuf::from("x.md"),
            "x.md".into(),
            String::new(),
        )
        .unwrap();

        assert_eq!(note.id.as_deref(), Some("ADR-0007"));
        assert_eq!(note.kind, NoteKind::Decision);
        assert_eq!(note.governs, vec!["src/**"]);
        assert_eq!(note.policy_pattern_ids, vec!["no-block-on"]);
        assert_eq!(note.targets_symbols, vec!["src/lib.rs#run"]);
        assert_eq!(note.target_pattern_refs[0].id, "ADR-0007/no-block-on");
        assert_eq!(note.target_pattern_refs[0].line, 16);
    }

    #[test]
    fn extracts_markdown_headings() {
        let headings = parse_markdown_headings("# One\ntext\n### Three ###\nnot heading");
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].0, 1);
        assert_eq!(headings[0].1, "One");
        assert_eq!(headings[1].2, 3);
    }

    #[test]
    fn shared_link_resolution_fixture_matches_vault() {
        #[derive(Deserialize)]
        struct Fixture {
            cases: Vec<Case>,
        }

        #[derive(Deserialize)]
        struct Case {
            target: String,
            source: Option<String>,
            pattern: Option<String>,
            note: Option<String>,
        }

        let fixture: Fixture =
            serde_json::from_str(include_str!("../fixtures/link-resolution.json")).unwrap();
        let root = unique_temp_dir("criv-link-fixtures");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs/adr")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]
"#,
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();
        std::fs::write(
            root.join("docs/adr/0001-local-cli-vault-architecture.md"),
            r#"---
id: ADR-0001
kind: decision
title: Local CLI vault architecture
status: accepted
policy:
  patterns:
    - id: no-block-on
---

# Local CLI vault architecture
"#,
        )
        .unwrap();

        let vault = Vault::load(&root).unwrap();
        for case in fixture.cases {
            match (
                vault.resolve_link(&case.target),
                case.source,
                case.pattern,
                case.note,
            ) {
                (ResolvedLink::Source { path, .. }, Some(expected), None, None) => {
                    assert_eq!(path, expected, "{}", case.target);
                }
                (ResolvedLink::Pattern { id }, None, Some(expected), None) => {
                    assert_eq!(id, expected, "{}", case.target);
                }
                (ResolvedLink::Note { id }, None, None, Some(expected)) => {
                    assert_eq!(id, expected, "{}", case.target);
                    assert!(vault.resolve_note(&expected).is_some(), "{}", case.target);
                }
                (ResolvedLink::Broken, None, None, None) => {}
                (actual, source, pattern, note) => {
                    panic!(
                        "unexpected resolution for {}: {:?}, expected source={:?} pattern={:?} note={:?}",
                        case.target, actual, source, pattern, note
                    );
                }
            }
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn disabled_source_index_skips_source_collection_and_graph() {
        let root = unique_temp_dir("criv-disabled-source-index");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs/adr")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]

[index]
source = false
"#,
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn run() {}\n").unwrap();

        let vault = Vault::load(&root).unwrap();

        assert!(vault.source_files().is_empty());
        assert!(vault.source_graph().files.is_empty());
        assert!(vault.source_index().entries().unwrap().is_empty());

        let _ = std::fs::remove_dir_all(root);
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()))
    }
}
