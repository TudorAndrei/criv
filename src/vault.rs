use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(test)]
use std::{cell::Cell, thread_local};

use serde::Deserialize;

use crate::Result;
use crate::config::Config;
use crate::source_graph::{SourceGraph, SourceGraphBuild};
use crate::source_index::{FffSourceIndex, SourceIndex};
use crate::util::{
    GlobMatcher, find_wiki_links_with_lines, is_adr_id, kebab,
    markdown_headings as parse_markdown_headings, read_to_string, strip_prefix, walk_files,
};
use crate::{c4, c4_artifact};

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct WorkCounts {
    source_target_resolutions: usize,
    link_source_resolutions: usize,
}

#[cfg(test)]
thread_local! {
    static WORK_COUNTS: Cell<WorkCounts> = const { Cell::new(WorkCounts {
        source_target_resolutions: 0,
        link_source_resolutions: 0,
    }) };
}

#[cfg(test)]
fn record_work(update: impl FnOnce(&mut WorkCounts)) {
    WORK_COUNTS.with(|counts| {
        let mut next = counts.get();
        update(&mut next);
        counts.set(next);
    });
}

#[cfg(test)]
pub(crate) fn reset_work_counts() {
    WORK_COUNTS.with(|counts| counts.set(WorkCounts::default()));
}

#[cfg(test)]
pub(crate) fn work_counts() -> WorkCounts {
    WORK_COUNTS.with(Cell::get)
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PolicyPattern {
    pub(crate) id: Option<String>,
    pub(crate) line: usize,
    pub(crate) language: Option<String>,
    pub(crate) pattern: Option<String>,
    pub(crate) rule: Option<String>,
    pub(crate) message: Option<String>,
}

impl PolicyPattern {
    pub(crate) fn has_inline_definition(&self) -> bool {
        self.language.is_some()
            || self.pattern.is_some()
            || self.rule.is_some()
            || self.message.is_some()
    }
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
    pub(crate) policy_patterns: Vec<PolicyPattern>,
    pub(crate) governs: Vec<String>,
    pub(crate) supersedes: Vec<String>,
    pub(crate) superseded_by: Vec<String>,
    pub(crate) wiki_links: Vec<WikiLink>,
    pub(crate) c4_diagrams: Vec<c4::C4Diagram>,
    pub(crate) frontmatter_error: Option<String>,
}

impl Note {
    pub(crate) fn display_id(&self) -> &str {
        self.id.as_deref().unwrap_or(&self.rel_path)
    }

    fn filename_stem(&self) -> Option<String> {
        self.path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
    }
}

#[derive(Debug)]
pub(crate) struct Vault {
    pub(crate) config: Config,
    pub(crate) notes: Vec<Note>,
    pub(crate) c4_artifacts: Vec<c4_artifact::C4Artifact>,
    note_ids: BTreeMap<String, usize>,
    filenames: BTreeMap<String, usize>,
    titles: BTreeMap<String, usize>,
    source_files: Vec<String>,
    source_index: Arc<dyn SourceIndex>,
    source_graph: SourceGraphBuild,
    patterns: BTreeSet<String>,
    link_resolutions: BTreeMap<String, ResolvedLink>,
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
        let cached = crate::source_graph::load_cached(root);
        Self::load_with_source_facilities(root, cached.as_ref(), None, true)
    }

    pub(crate) fn load_docs_only(root: &Path) -> Result<Self> {
        Self::load_with_source_facilities(root, None, None, false)
    }

    pub(crate) fn load_incremental_with_source_index(
        root: &Path,
        previous_graph: Option<&SourceGraphBuild>,
        shared_source_index: Option<Arc<dyn SourceIndex>>,
    ) -> Result<Self> {
        Self::load_with_source_facilities(root, previous_graph, shared_source_index, true)
    }

    fn load_with_source_facilities(
        root: &Path,
        previous_graph: Option<&SourceGraphBuild>,
        shared_source_index: Option<Arc<dyn SourceIndex>>,
        load_sources: bool,
    ) -> Result<Self> {
        let config = Config::load(root)?;
        let docs_path = config.docs_path(root);
        let notes = walk_files(&docs_path, Some("md"))?
            .into_iter()
            .map(|path| parse_note(root, &docs_path, &path))
            .collect::<Result<Vec<_>>>()?;
        let c4_artifacts = walk_files(&docs_path, Some("c4"))?
            .into_iter()
            .map(|path| c4_artifact::parse_file(root, &docs_path, &path))
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

        let patterns = registered_policy_patterns(&notes);

        let (source_files, source_index, source_graph): (
            Vec<String>,
            Arc<dyn SourceIndex>,
            SourceGraphBuild,
        ) = if load_sources && config.source_index {
            let source_index: Arc<dyn SourceIndex> = match shared_source_index {
                Some(source_index) => source_index,
                None => Arc::new(FffSourceIndex::new(
                    root,
                    &config.source_roots,
                    &config.source_exclude,
                    false,
                )?),
            };
            let source_files = source_index
                .entries()?
                .into_iter()
                .map(|entry| entry.path)
                .collect::<Vec<_>>();
            let source_graph =
                SourceGraphBuild::build_incremental(root, &source_files, previous_graph)?
                    .publish(root)?;
            (source_files, source_index, source_graph)
        } else {
            (
                Vec::new(),
                Arc::new(EmptySourceIndex),
                SourceGraphBuild::disabled(),
            )
        };

        let mut vault = Self {
            config,
            notes,
            c4_artifacts,
            note_ids,
            filenames,
            titles,
            source_files,
            source_index,
            source_graph,
            patterns,
            link_resolutions: BTreeMap::new(),
        };
        vault.index_link_resolutions();
        Ok(vault)
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
        let target = normalized_link_target(target);
        self.link_resolutions
            .get(target)
            .cloned()
            .unwrap_or_else(|| self.resolve_link_uncached(target))
    }

    fn resolve_link_uncached(&self, target: &str) -> ResolvedLink {
        if is_typed_source_target(target) {
            return self.resolve_source_link(target);
        }

        if let Some(pattern_id) = pattern_link_id(target) {
            if self.resolve_policy_pattern(pattern_id).is_some() {
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

        self.resolve_source_link(target)
    }

    fn resolve_source_link(&self, target: &str) -> ResolvedLink {
        #[cfg(test)]
        record_work(|counts| counts.link_source_resolutions += 1);

        match self.resolve_source_target(target) {
            SourceTargetResolution::Resolved { path, ambiguous } => {
                ResolvedLink::Source { path, ambiguous }
            }
            SourceTargetResolution::MissingFile
            | SourceTargetResolution::MissingFragment { .. } => ResolvedLink::Broken,
        }
    }

    fn index_link_resolutions(&mut self) {
        let targets = self
            .notes
            .iter()
            .flat_map(|note| note.wiki_links.iter())
            .map(|link| normalized_link_target(&link.target).to_string())
            .collect::<BTreeSet<_>>();
        self.link_resolutions = targets
            .into_iter()
            .map(|target| {
                let resolution = self.resolve_link_uncached(&target);
                (target, resolution)
            })
            .collect();
    }

    pub(crate) fn resolve_source_path(&self, path: &str) -> Option<(String, bool)> {
        self.source_index.resolve_partial_path(path)
    }

    pub(crate) fn resolve_source_target(&self, target: &str) -> SourceTargetResolution {
        #[cfg(test)]
        record_work(|counts| counts.source_target_resolutions += 1);

        let target = source_target_body(target);
        let Some((path, ambiguous)) = self.resolve_source_path(source_fragment_path(target)) else {
            return SourceTargetResolution::MissingFile;
        };
        let Some(fragment) = source_fragment_name(target) else {
            return SourceTargetResolution::Resolved { path, ambiguous };
        };

        if self
            .source_graph
            .graph()
            .resolve_symbol(&format!("{path}#{fragment}"))
            .is_some()
        {
            SourceTargetResolution::Resolved { path, ambiguous }
        } else {
            SourceTargetResolution::MissingFragment { path }
        }
    }

    pub(crate) fn canonical_source_target(&self, target: &str) -> Option<String> {
        let target = source_target_body(target);
        let (path, _) = self.resolve_source_path(source_fragment_path(target))?;
        self.canonical_source_target_for_path(target, &path)
    }

    pub(crate) fn canonical_source_target_for_path(
        &self,
        target: &str,
        path: &str,
    ) -> Option<String> {
        let target = source_target_body(target);
        let Some(fragment) = source_fragment_name(target) else {
            return Some(path.to_string());
        };
        self.source_graph
            .graph()
            .canonical_symbol_target(&format!("{path}#{fragment}"))
    }

    pub(crate) fn source_files_matching_glob(&self, pattern: &str) -> Vec<String> {
        self.source_files_matching_globs(&[pattern.to_string()])
    }

    pub(crate) fn source_files_matching_globs(&self, patterns: &[String]) -> Vec<String> {
        let matcher = GlobMatcher::from_valid_patterns(patterns);
        let mut matches_by_pattern = vec![Vec::new(); patterns.len()];
        let mut indices = Vec::new();
        for source_file in &self.source_files {
            matcher.matching_pattern_indices_into(source_file, &mut indices);
            for index in &indices {
                matches_by_pattern[*index].push(source_file.clone());
            }
        }
        let mut matches = Vec::new();
        for (index, pattern) in patterns.iter().enumerate() {
            if matches_by_pattern[index].is_empty()
                && let SourceTargetResolution::Resolved { path, .. } =
                    self.resolve_source_target(pattern)
            {
                matches.push(path);
            } else {
                matches.extend(matches_by_pattern[index].iter().cloned());
            }
        }
        matches
    }

    pub(crate) fn source_globs_have_matches(&self, patterns: &[String]) -> Vec<bool> {
        let matcher = GlobMatcher::from_valid_patterns(patterns);
        let mut matched = vec![false; patterns.len()];
        let mut indices = Vec::new();
        for source_file in &self.source_files {
            matcher.matching_pattern_indices_into(source_file, &mut indices);
            for index in &indices {
                matched[*index] = true;
            }
        }
        for (index, pattern) in patterns.iter().enumerate() {
            matched[index] |= matches!(
                self.resolve_source_target(pattern),
                SourceTargetResolution::Resolved { .. }
            );
        }
        matched
    }

    pub(crate) fn source_files(&self) -> &[String] {
        &self.source_files
    }

    pub(crate) fn source_graph(&self) -> &SourceGraph {
        self.source_graph.graph()
    }

    pub(crate) fn source_graph_build(&self) -> &SourceGraphBuild {
        &self.source_graph
    }

    pub(crate) fn retain_source_graph_changes_from(&mut self, previous: &SourceGraphBuild) {
        self.source_graph.retain_changed_files_from(previous);
    }

    pub(crate) fn source_index(&self) -> &dyn SourceIndex {
        self.source_index.as_ref()
    }

    pub(crate) fn patterns(&self) -> &BTreeSet<String> {
        &self.patterns
    }

    /// Resolves the canonical full ID of an inline ADR policy pattern.
    ///
    /// Pattern IDs deliberately do not use general note resolution: a named
    /// pattern is owned by an ADR and must be addressed as `ADR-NNNN/local-id`.
    pub(crate) fn resolve_policy_pattern(
        &self,
        pattern_id: &str,
    ) -> Option<(&Note, &PolicyPattern)> {
        let (adr_id, local_id) = pattern_id.split_once('/')?;
        if !is_adr_id(adr_id) || local_id.trim().is_empty() {
            return None;
        }
        let note = self
            .note_ids
            .get(&adr_id.to_lowercase())
            .and_then(|index| self.notes.get(*index))?;
        if note.kind != NoteKind::Decision {
            return None;
        }
        let policy = note
            .policy_patterns
            .iter()
            .find(|policy| policy.id.as_deref() == Some(local_id))?;
        Some((note, policy))
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
        let patterns = registered_policy_patterns(&notes);

        let mut vault = Self {
            config: Config::default(),
            notes,
            c4_artifacts: Vec::new(),
            note_ids,
            filenames,
            titles,
            source_files: Vec::new(),
            source_index: Arc::new(EmptySourceIndex),
            source_graph: SourceGraphBuild::disabled(),
            patterns,
            link_resolutions: BTreeMap::new(),
        };
        vault.index_link_resolutions();
        vault
    }
}

/// Returns the policy IDs published in generated state.
///
/// Policy lookup remains status-agnostic for validation, search, and wikilink
/// resolution. State registration is deliberately narrower: only policies
/// owned by an accepted ADR are published.
fn registered_policy_patterns(notes: &[Note]) -> BTreeSet<String> {
    notes
        .iter()
        .filter(|note| {
            note.kind == NoteKind::Decision && note.status.as_deref() == Some("accepted")
        })
        .filter_map(|note| note.id.as_deref().map(|id| (id, note)))
        .flat_map(|(id, note)| {
            note.policy_patterns.iter().filter_map(move |pattern| {
                pattern
                    .id
                    .as_deref()
                    .map(|local_id| format!("{id}/{local_id}"))
            })
        })
        .collect()
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
    let (frontmatter, body, frontmatter_lines) = split_frontmatter(&contents);
    let doc_rel_path = strip_prefix(path, docs_path);
    // Positions are file-relative (ADR-0045). The frontmatter offset applies to
    // the parsed-note body only; the error branch below keeps the whole file as
    // the body, so it needs no offset at all.
    let mut line_offset = frontmatter_lines;
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
            policy_patterns: Vec::new(),
            governs: Vec::new(),
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            wiki_links: Vec::new(),
            c4_diagrams: Vec::new(),
            frontmatter_error: Some(err),
        },
    };
    if note.frontmatter_error.is_some() {
        line_offset = 0;
    }
    note.wiki_links = find_wiki_links_with_lines(&note.body)
        .into_iter()
        .map(|(line, raw)| WikiLink {
            target: raw.split('|').next().unwrap_or(&raw).trim().to_string(),
            raw,
            line: line + line_offset,
        })
        .collect();
    note.headings = parse_markdown_headings(&note.body)
        .into_iter()
        .map(|(level, text, line)| Heading {
            level,
            text,
            line: line + line_offset,
        })
        .collect();
    note.c4_diagrams = c4::parse_diagrams(&note.body, line_offset);
    note.rel_path = rel_path;
    Ok(note)
}

/// Splits a note into its frontmatter block and its body, plus the number of
/// lines the frontmatter consumed — both `---` delimiters included. Callers add
/// that count back to body-relative positions so every line they report is
/// file-relative (see ADR-0045).
fn split_frontmatter(contents: &str) -> (&str, String, usize) {
    let Some((opening, frontmatter_start)) = delimiter_line(contents, 0) else {
        return ("", contents.to_string(), 0);
    };
    if opening != "---" {
        return ("", contents.to_string(), 0);
    }

    let mut cursor = frontmatter_start;
    while let Some((line, next)) = delimiter_line(contents, cursor) {
        if line == "---" {
            let consumed = contents[..next].matches('\n').count();
            return (
                &contents[frontmatter_start..cursor],
                contents[next..].to_string(),
                consumed,
            );
        }
        cursor = next;
    }

    ("", contents.to_string(), 0)
}

/// Returns the line without its LF/CRLF terminator plus the next byte offset.
/// A final unterminated line is deliberately not a frontmatter delimiter.
fn delimiter_line(contents: &str, start: usize) -> Option<(&str, usize)> {
    let newline = contents[start..].find('\n')? + start;
    let line_end = contents[start..newline]
        .strip_suffix('\r')
        .map_or(newline, |_| newline - 1);
    Some((&contents[start..line_end], newline + 1))
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

    let policy_patterns: Vec<PolicyPattern> = raw
        .policy
        .map(|policy| {
            policy
                .patterns
                .into_iter()
                .map(|pattern| {
                    let line = raw_pattern_line(frontmatter, &pattern);
                    PolicyPattern {
                        id: pattern.id,
                        line,
                        language: pattern.language,
                        pattern: pattern.pattern,
                        rule: pattern.rule,
                        message: pattern.message,
                    }
                })
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
        policy_patterns,
        governs: raw.governs,
        supersedes: raw.supersedes,
        superseded_by: raw.superseded_by,
        wiki_links: Vec::new(),
        c4_diagrams: Vec::new(),
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

fn normalized_link_target(target: &str) -> &str {
    target.split('|').next().unwrap_or(target).trim()
}

pub(crate) fn source_fragment_path(value: &str) -> &str {
    let value = source_target_body(value);
    value.split('#').next().unwrap_or(value)
}

pub(crate) fn source_fragment_name(value: &str) -> Option<&str> {
    let value = source_target_body(value);
    let fragment = value.split_once('#')?.1;
    (!fragment.is_empty() && !is_line_fragment(fragment)).then_some(fragment)
}

pub(crate) fn source_target_body(value: &str) -> &str {
    value
        .split('|')
        .next()
        .unwrap_or(value)
        .trim()
        .strip_prefix("source:")
        .unwrap_or_else(|| value.split('|').next().unwrap_or(value).trim())
}

pub(crate) fn is_typed_source_target(value: &str) -> bool {
    value
        .split('|')
        .next()
        .unwrap_or(value)
        .trim()
        .starts_with("source:")
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

fn raw_pattern_line(frontmatter: &str, pattern: &RawPatternRef) -> usize {
    pattern
        .id
        .as_deref()
        .or(pattern.reference.as_deref())
        .or(pattern.pattern.as_deref())
        .or(pattern.rule.as_deref())
        .or(pattern.language.as_deref())
        .or(pattern.message.as_deref())
        .map(|needle| frontmatter_line(frontmatter, needle))
        .unwrap_or_else(|| frontmatter_line(frontmatter, "patterns:"))
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
    language: Option<String>,
    pattern: Option<String>,
    rule: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawPolicy {
    patterns: Vec<RawPatternRef>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_index::FffSourceIndex;
    use std::sync::Arc;

    #[test]
    fn splits_exact_frontmatter_delimiters_with_lf_and_crlf() {
        for contents in [
            "---\nid: doc\n---\n# Body\n",
            "---\r\nid: doc\r\n---\r\n# Body\r\n",
            "---\r\nid: doc\n---\r\n# Body\n",
        ] {
            let (frontmatter, body, frontmatter_lines) = split_frontmatter(contents);
            assert!(frontmatter.contains("id: doc"));
            assert!(body.starts_with("# Body"));
            assert_eq!(frontmatter_lines, 3, "both delimiters and the field line");
        }
    }

    #[test]
    fn ignores_delimiter_like_or_unclosed_frontmatter() {
        for contents in [
            "---\nid: doc\n---suffix\n# Body\n",
            "---\nid: doc\n# Body\n",
            "\u{feff}---\nid: doc\n---\n# Body\n",
        ] {
            assert_eq!(split_frontmatter(contents), ("", contents.to_string(), 0));
        }
    }

    #[test]
    fn supports_empty_frontmatter() {
        assert_eq!(
            split_frontmatter("---\n---\nbody\n"),
            ("", "body\n".into(), 2)
        );
    }

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
      language: rust
      pattern: "$RT.block_on($$$ARGS)"
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
        assert_eq!(note.policy_patterns.len(), 1);
        assert_eq!(note.policy_patterns[0].id.as_deref(), Some("no-block-on"));
        assert_eq!(note.targets_symbols, vec!["src/lib.rs#run"]);
        assert_eq!(note.target_pattern_refs[0].id, "ADR-0007/no-block-on");
        assert_eq!(note.target_pattern_refs[0].line, 18);
    }

    #[test]
    fn parses_inline_policy_pattern_definitions() {
        let note = parse_frontmatter(
            r#"id: ADR-0042
kind: decision
title: Inline policy
status: accepted
policy:
  patterns:
    - id: no-println
      language: rust
      pattern: "println!($$$ARGS)"
      message: Use diagnostics instead.
    - id: no-block-on
      language: rust
      rule: |
        all:
          - pattern: "$RT.block_on($$$ARGS)"
"#,
            PathBuf::from("x.md"),
            "x.md".into(),
            String::new(),
        )
        .unwrap();

        assert_eq!(note.policy_patterns.len(), 2);
        assert_eq!(note.policy_patterns[0].id.as_deref(), Some("no-println"));
        assert_eq!(note.policy_patterns[0].language.as_deref(), Some("rust"));
        assert_eq!(
            note.policy_patterns[0].pattern.as_deref(),
            Some("println!($$$ARGS)")
        );
        assert_eq!(
            note.policy_patterns[0].message.as_deref(),
            Some("Use diagnostics instead.")
        );
        assert_eq!(
            note.policy_patterns[1].rule.as_deref(),
            Some("all:\n  - pattern: \"$RT.block_on($$$ARGS)\"\n")
        );
    }

    #[test]
    fn registers_only_accepted_decision_policy_patterns_but_resolves_all_decisions() {
        let accepted = parsed_note(
            "criv-accepted-policy-registration",
            r#"---
id: ADR-0001
kind: decision
title: Accepted
status: accepted
policy:
  patterns:
    - id: accepted
---

# Accepted
"#,
        );
        let draft = parsed_note(
            "criv-draft-policy-registration",
            r#"---
id: ADR-0002
kind: decision
title: Draft
status: draft
policy:
  patterns:
    - id: draft
---

# Draft
"#,
        );
        let missing_status = parsed_note(
            "criv-missing-policy-registration",
            r#"---
id: ADR-0003
kind: decision
title: Missing status
policy:
  patterns:
    - id: missing-status
---

# Missing status
"#,
        );
        let non_decision = parsed_note(
            "criv-doc-policy-registration",
            r#"---
id: DOC-0001
kind: doc
title: Documentation
status: accepted
policy:
  patterns:
    - id: documentation
---

# Documentation
"#,
        );
        let vault = Vault::from_parts_for_test(vec![accepted, draft, missing_status, non_decision]);

        assert_eq!(
            vault.patterns(),
            &BTreeSet::from(["ADR-0001/accepted".to_string()])
        );
        assert!(vault.resolve_policy_pattern("ADR-0002/draft").is_some());
        assert_eq!(
            vault.resolve_link("match:ADR-0002/draft"),
            ResolvedLink::Pattern {
                id: "ADR-0002/draft".into()
            }
        );
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
            ambiguous: Option<bool>,
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
        std::fs::write(root.join("src/helper.rs"), "fn help() {}\n").unwrap();
        std::fs::create_dir_all(root.join("src/one")).unwrap();
        std::fs::create_dir_all(root.join("src/two")).unwrap();
        std::fs::write(root.join("src/one/shared.rs"), "fn one() {}\n").unwrap();
        std::fs::write(root.join("src/two/shared.rs"), "fn two() {}\n").unwrap();
        std::fs::write(
            root.join("docs/lib.rs.md"),
            r#"---
id: collision-note
kind: doc
title: Collision note
---

# Collision note

## Existing heading
"#,
        )
        .unwrap();
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
      language: rust
      pattern: "println!($$$ARGS)"
---

# Local CLI vault architecture

See [[lib.rs]], [[match:ADR-0001/no-block-on]], [[helper.rs#help]],
[[helper.rs#help]], and [[source:lib.rs#run]].
"#,
        )
        .unwrap();

        reset_work_counts();
        let vault = Vault::load(&root).unwrap();
        assert_eq!(work_counts().link_source_resolutions, 2);

        let _ = crate::check::validate_with_previous_state(&vault, None);
        let state = crate::state::State::build(&root, &vault).unwrap();
        assert_eq!(work_counts().link_source_resolutions, 2);
        let state = serde_json::to_value(state).unwrap();
        let edges = state["graph"]["edges"].as_array().unwrap();
        for (kind, target) in [
            ("cites", "note:collision-note"),
            ("references", "code:src/helper.rs"),
            ("references", "pattern:ADR-0001/no-block-on"),
        ] {
            assert!(edges.iter().any(|edge| {
                edge["from"] == "note:ADR-0001" && edge["kind"] == kind && edge["to"] == target
            }));
        }

        reset_work_counts();
        assert!(matches!(
            vault.resolve_link("collision-note"),
            ResolvedLink::Note { .. }
        ));
        assert!(matches!(
            vault.resolve_link("ADR-0001#match:ADR-0001/no-block-on"),
            ResolvedLink::Pattern { .. }
        ));
        assert_eq!(work_counts().link_source_resolutions, 0);

        for case in fixture.cases {
            match (
                vault.resolve_link(&case.target),
                case.source,
                case.ambiguous,
                case.pattern,
                case.note,
            ) {
                (
                    ResolvedLink::Source { path, ambiguous },
                    Some(expected),
                    Some(expected_ambiguous),
                    None,
                    None,
                ) => {
                    assert_eq!(path, expected, "{}", case.target);
                    assert_eq!(ambiguous, expected_ambiguous, "{}", case.target);
                }
                (ResolvedLink::Pattern { id }, None, None, Some(expected), None) => {
                    assert_eq!(id, expected, "{}", case.target);
                }
                (ResolvedLink::Note { id }, None, None, None, Some(expected)) => {
                    assert_eq!(id, expected, "{}", case.target);
                    assert!(vault.resolve_note(&expected).is_some(), "{}", case.target);
                }
                (ResolvedLink::Broken, None, None, None, None) => {}
                (actual, source, ambiguous, pattern, note) => {
                    panic!(
                        "unexpected resolution for {}: {:?}, expected source={:?} ambiguous={:?} pattern={:?} note={:?}",
                        case.target, actual, source, ambiguous, pattern, note
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

    #[test]
    fn vault_builds_its_graph_from_the_injected_source_index() {
        let root = unique_temp_dir("criv-injected-source-index");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src"]
"#,
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();

        let index: Arc<dyn SourceIndex> =
            Arc::new(FffSourceIndex::new(&root, &["src".into()], &[], false).unwrap());
        let expected = index
            .entries()
            .unwrap()
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();
        let vault = Vault::load_incremental_with_source_index(&root, None, Some(index)).unwrap();

        assert_eq!(vault.source_files(), expected);
        assert!(vault.source_graph().files.contains_key("src/lib.rs"));
        reset_work_counts();
        assert_eq!(
            vault.resolve_source_target("src/lib.rs#run"),
            SourceTargetResolution::Resolved {
                path: "src/lib.rs".into(),
                ambiguous: false,
            }
        );
        assert_eq!(work_counts().source_target_resolutions, 1);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn batch_source_globs_preserve_order_fallbacks_and_invalid_patterns() {
        let root = unique_temp_dir("criv-batch-source-globs");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            "[source]\nroots = [\"src\", \"assets/blob.bin\"]\n",
        )
        .unwrap();
        std::fs::write(root.join("src/a.rs"), "pub fn a() {}\n").unwrap();
        std::fs::write(root.join("src/z.rs"), "pub fn run() {}\n").unwrap();
        std::fs::write(root.join("assets/blob.bin"), [0, 0, 0, 0]).unwrap();

        let vault = Vault::load(&root).unwrap();
        assert_eq!(
            vault.source_files_matching_globs(&["src/z.rs#run".into(), "src/a.rs".into()]),
            vec!["src/z.rs", "src/a.rs"]
        );
        assert_eq!(
            vault.source_files_matching_globs(&["src/*.rs".into(), "src/a.rs".into()]),
            vec!["src/a.rs", "src/z.rs", "src/a.rs"]
        );
        assert_eq!(
            vault.source_files_matching_globs(&["[".into(), "src/a.rs".into()]),
            vec!["src/a.rs"]
        );
        assert_eq!(
            vault.source_files_matching_globs(&["assets/blob.bin".into()]),
            vec!["assets/blob.bin"]
        );

        let _ = std::fs::remove_dir_all(root);
    }

    /// Returns the 1-based line of the first line whose content equals `needle`.
    fn real_line(contents: &str, needle: &str) -> usize {
        contents
            .lines()
            .position(|line| line.trim_end() == needle)
            .expect("needle must appear in the fixture")
            + 1
    }

    fn parsed_note(prefix: &str, contents: &str) -> Note {
        let root = unique_temp_dir(prefix);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("note.md");
        std::fs::write(&path, contents).unwrap();
        let note = parse_note(&root, &root, &path).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        note
    }

    #[test]
    fn note_positions_are_file_relative_with_frontmatter() {
        let contents =
            "---\nid: DOC-1\nkind: doc\ntitle: Doc\n---\n\n# Heading\n\nSee [[other-note]].\n";
        let note = parsed_note("criv-note-lines-frontmatter", contents);

        assert_eq!(note.headings[0].line, real_line(contents, "# Heading"));
        assert_eq!(
            note.wiki_links[0].line,
            real_line(contents, "See [[other-note]].")
        );
    }

    #[test]
    fn note_positions_are_file_relative_without_frontmatter() {
        let contents = "# Heading\n\nSee [[other-note]].\n";
        let note = parsed_note("criv-note-lines-bare", contents);

        assert_eq!(note.headings[0].line, real_line(contents, "# Heading"));
        assert_eq!(
            note.wiki_links[0].line,
            real_line(contents, "See [[other-note]].")
        );
    }

    #[test]
    fn note_positions_are_file_relative_when_frontmatter_fails_to_parse() {
        // The error branch keeps the whole file as the body, so no offset
        // applies — adding one would push every position past its real line.
        let contents = "---\nid: [unclosed\n---\n\n# Heading\n\nSee [[other-note]].\n";
        let note = parsed_note("criv-note-lines-bad-frontmatter", contents);

        assert!(note.frontmatter_error.is_some());
        // The unparsed frontmatter stays in the body, so its closing `---` also
        // reads as a setext heading here; pick the real ATX heading by text.
        let heading = note
            .headings
            .iter()
            .find(|heading| heading.text == "Heading")
            .expect("the ATX heading must be parsed");
        assert_eq!(heading.line, real_line(contents, "# Heading"));
        assert_eq!(
            note.wiki_links[0].line,
            real_line(contents, "See [[other-note]].")
        );
    }

    #[test]
    fn c4_diagram_positions_are_file_relative() {
        let contents = "---\nid: DOC-1\nkind: doc\ntitle: Doc\n---\n\n```mermaid\nC4Context\nPerson(user, \"User\")\n```\n";
        let note = parsed_note("criv-note-lines-c4", contents);

        assert_eq!(note.c4_diagrams.len(), 1);
        assert_eq!(
            note.c4_diagrams[0].elements[0].line,
            real_line(contents, "Person(user, \"User\")")
        );
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()))
    }
}
