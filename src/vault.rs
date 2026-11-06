use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;

use crate::Result;
use crate::config::Config;
use crate::diagnostic::SourceLocation;
use crate::discovery::{discover_vault, read_selected_text_from};
use crate::glob::GlobMatcher;
use crate::identity::{pattern_id, slug, split_pattern_id, strip_prefix};
use crate::markdown::{
    LinkTarget, find_wiki_links_with_lines, line_index, line_number,
    markdown_headings as parse_markdown_headings, split_frontmatter,
};
use crate::repository::RepositoryFiles;
use crate::source::{SourceGraph, SourceState};
use crate::structural::{PolicyLanguage, PolicyPattern};
#[cfg(test)]
use crate::work::{self, Work};
use criv_state_wire::{asset, content_hash};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NoteKind {
    Doc,
    Decision,
    #[default]
    Unknown,
}

impl NoteKind {
    fn parse(kind: Option<&str>) -> Self {
        match kind {
            Some("doc") => Self::Doc,
            Some("decision") => Self::Decision,
            _ => Self::Unknown,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Doc => "doc",
            Self::Decision => "decision",
            Self::Unknown => "unknown",
        }
    }
}

/// The frontmatter `status` of a note; text outside the four ADR statuses
/// is `Unknown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionStatus {
    Draft,
    Accepted,
    Rejected,
    Superseded,
    Unknown,
}

impl DecisionStatus {
    fn parse(status: &str) -> Self {
        match status {
            "draft" => Self::Draft,
            "accepted" => Self::Accepted,
            "rejected" => Self::Rejected,
            "superseded" => Self::Superseded,
            _ => Self::Unknown,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Superseded => "superseded",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PatternRef {
    pub(crate) id: String,
    pub(crate) line: usize,
}

#[derive(Debug, Clone)]
pub struct WikiLink {
    pub(crate) raw: String,
    pub(crate) target: String,
    pub(crate) line: usize,
    pub(crate) location: Option<SourceLocation>,
}

#[derive(Debug, Clone)]
pub struct Heading {
    pub(crate) level: usize,
    pub(crate) text: String,
    pub(crate) line: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Note {
    path: PathBuf,
    rel_path: String,
    id: Option<String>,
    kind: NoteKind,
    title: Option<String>,
    status: Option<DecisionStatus>,
    body: String,
    headings: Vec<Heading>,
    targets_symbols: Vec<String>,
    targets_scope: Vec<String>,
    target_pattern_refs: Vec<PatternRef>,
    target_pattern_ids: Vec<String>,
    policy_patterns: Vec<PolicyPattern>,
    governs: Vec<String>,
    supersedes: Vec<String>,
    superseded_by: Vec<String>,
    wiki_links: Vec<WikiLink>,
    frontmatter_error: Option<String>,
    frontmatter_error_location: Option<SourceLocation>,
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

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// The repository-relative path.
    pub(crate) fn rel_path(&self) -> &str {
        &self.rel_path
    }

    pub(crate) fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub(crate) const fn kind(&self) -> NoteKind {
        self.kind
    }

    pub(crate) fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub(crate) const fn status(&self) -> Option<DecisionStatus> {
        self.status
    }

    pub(crate) fn body(&self) -> &str {
        &self.body
    }

    pub(crate) fn headings(&self) -> &[Heading] {
        &self.headings
    }

    pub(crate) fn targets_symbols(&self) -> &[String] {
        &self.targets_symbols
    }

    pub(crate) fn targets_scope(&self) -> &[String] {
        &self.targets_scope
    }

    pub(crate) fn target_pattern_refs(&self) -> &[PatternRef] {
        &self.target_pattern_refs
    }

    pub(crate) fn target_pattern_ids(&self) -> &[String] {
        &self.target_pattern_ids
    }

    pub(crate) fn policy_patterns(&self) -> &[PolicyPattern] {
        &self.policy_patterns
    }

    /// The `governs` globs as written.
    pub(crate) fn governs(&self) -> &[String] {
        &self.governs
    }

    /// The `governs` globs, where a decision without any governs `**`.
    pub(crate) fn effective_governs(&self) -> Cow<'_, [String]> {
        if self.kind == NoteKind::Decision && self.governs.is_empty() {
            Cow::Owned(vec!["**".into()])
        } else {
            Cow::Borrowed(&self.governs)
        }
    }

    pub(crate) fn supersedes(&self) -> &[String] {
        &self.supersedes
    }

    pub(crate) fn superseded_by(&self) -> &[String] {
        &self.superseded_by
    }

    pub(crate) fn wiki_links(&self) -> &[WikiLink] {
        &self.wiki_links
    }

    pub(crate) fn frontmatter_error(&self) -> Option<&str> {
        self.frontmatter_error.as_deref()
    }

    pub(crate) const fn frontmatter_error_location(&self) -> Option<&SourceLocation> {
        self.frontmatter_error_location.as_ref()
    }
}

/// How [`Vault::load`] reads configuration and Source.
#[derive(Debug, Default)]
pub struct LoadOptions {
    /// The configuration to use instead of reading `criv.toml`.
    pub config: Option<Config>,
    pub source: SourceLoad,
}

#[derive(Debug, Default)]
pub enum SourceLoad {
    /// Observe Source now.
    #[default]
    Refresh,
    /// Use a Source observation the caller already made.
    Observed(SourceState),
    /// Load documents only.
    Skip,
}

#[derive(Debug)]
pub struct Vault {
    files: RepositoryFiles,
    config: Config,
    notes: Vec<Note>,
    assets: Vec<DocumentationAsset>,
    note_ids: BTreeMap<String, usize>,
    filenames: BTreeMap<String, usize>,
    titles: BTreeMap<String, usize>,
    decisions: BTreeMap<String, usize>,
    source: SourceState,
    effective_decisions: BTreeSet<String>,
    patterns: BTreeSet<String>,
    link_resolutions: BTreeMap<String, ResolvedLink>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentationAsset {
    pub(crate) path: String,
    pub(crate) mime: String,
    pub(crate) bytes: u64,
    pub(crate) hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedLink {
    Source { path: String, ambiguous: bool },
    Pattern { id: String },
    Note { id: String },
    Broken,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceTargetResolution {
    Resolved { path: String, ambiguous: bool },
    MissingFile,
    MissingFragment { path: String },
}

impl Vault {
    pub(crate) fn load(files: &RepositoryFiles, options: LoadOptions) -> Result<Self> {
        let root = files.root();
        let config = options
            .config
            .map_or_else(|| Config::load_from(files), Ok)?;
        let discovered = discover_vault(root, &config.docs_dir)?;
        let notes = discovered
            .markdown
            .into_iter()
            .map(|path| parse_note(files, &root.join(path)))
            .collect::<Result<Vec<_>>>()?;
        let assets = load_documentation_assets(
            files,
            discovered.assets,
            asset::MAX_BYTES,
            asset::MAX_TOTAL_BYTES,
        )?;

        let mut note_ids = BTreeMap::new();
        let mut filenames = BTreeMap::new();
        let mut titles = BTreeMap::new();
        let mut decisions = BTreeMap::new();
        for (index, note) in notes.iter().enumerate() {
            if let Some(id) = &note.id {
                note_ids.entry(id.to_lowercase()).or_insert(index);
                if note.kind == NoteKind::Decision {
                    decisions.insert(id.clone(), index);
                }
            }
            if let Some(stem) = note.filename_stem() {
                filenames.entry(stem.to_lowercase()).or_insert(index);
            }
            if let Some(title) = &note.title {
                titles.entry(title.to_lowercase()).or_insert(index);
            }
        }

        let effective_decisions = effective_accepted_decision_ids(&notes, &decisions);
        let patterns = registered_policy_patterns(&notes, &effective_decisions);

        let source = match options.source {
            SourceLoad::Refresh => SourceState::refresh(files, &config, None)?,
            SourceLoad::Observed(source) => source,
            SourceLoad::Skip => SourceState::disabled(),
        };

        let mut vault = Self {
            files: files.clone(),
            config,
            notes,
            assets,
            note_ids,
            filenames,
            titles,
            decisions,
            source,
            effective_decisions,
            patterns,
            link_resolutions: BTreeMap::new(),
        };
        vault.index_link_resolutions();
        Ok(vault)
    }

    pub(crate) const fn config(&self) -> &Config {
        &self.config
    }

    pub(crate) fn notes(&self) -> &[Note] {
        &self.notes
    }

    /// The decision note with exactly this id; the last one when ids repeat.
    pub(crate) fn decision(&self, id: &str) -> Option<&Note> {
        self.decisions
            .get(id)
            .and_then(|index| self.notes.get(*index))
    }

    /// Every decision note by id, as [`Self::decision`] resolves it.
    pub(crate) fn decisions(&self) -> impl Iterator<Item = (&str, &Note)> {
        self.decisions
            .iter()
            .filter_map(|(id, index)| self.notes.get(*index).map(|note| (id.as_str(), note)))
    }

    /// The Source files that the effective `governs` globs of `note` select.
    pub(crate) fn governed_files(&self, note: &Note) -> Vec<String> {
        self.source_files_matching_globs(&note.effective_governs())
    }

    pub(crate) fn resolve_note(&self, target: &str) -> Option<&Note> {
        let key = target.to_lowercase();
        self.note_ids
            .get(&key)
            .or_else(|| self.filenames.get(&key))
            .or_else(|| self.titles.get(&key))
            .and_then(|index| self.notes.get(*index))
    }

    pub(crate) const fn repository_files(&self) -> &RepositoryFiles {
        &self.files
    }

    pub(crate) fn is_file_backed_note_target(&self, target: &str) -> bool {
        let target_without_heading = LinkTarget::parse(target).path();
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
        let link = LinkTarget::parse(target);
        let target = link.as_str();
        let note = self.resolve_note(link.path())?;
        let mut portable = note.filename_stem()?;
        if let Some(heading) = link.fragment() {
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
        let target = LinkTarget::parse(target).as_str();
        self.link_resolutions
            .get(target)
            .cloned()
            .unwrap_or_else(|| self.resolve_link_uncached(target))
    }

    fn resolve_link_uncached(&self, target: &str) -> ResolvedLink {
        let link = LinkTarget::parse(target);
        if link.is_source() {
            return self.resolve_source_link(target);
        }

        if let Some(pattern_id) = link.pattern_id() {
            if self.resolve_policy_pattern(pattern_id).is_some() {
                return ResolvedLink::Pattern {
                    id: pattern_id.to_string(),
                };
            }
            return ResolvedLink::Broken;
        }

        if let Some(note) = self.resolve_note(link.path()) {
            if let Some(heading) = link.fragment()
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
        work::record(Work::LinkSourceResolution);

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
            .map(|link| LinkTarget::parse(&link.target).as_str().to_string())
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
        if LinkTarget::parse(path).pattern_id().is_some() {
            return None;
        }
        self.source.resolve_partial_path(path)
    }

    pub(crate) fn resolve_source_target(&self, target: &str) -> SourceTargetResolution {
        #[cfg(test)]
        work::record(Work::SourceTargetResolution);

        let link = LinkTarget::parse(target);
        let Some((path, ambiguous)) = self.resolve_source_path(link.path()) else {
            return SourceTargetResolution::MissingFile;
        };
        let Some(fragment) = link.symbol() else {
            return SourceTargetResolution::Resolved { path, ambiguous };
        };

        if self
            .source
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
        let (path, _) = self.resolve_source_path(LinkTarget::parse(target).path())?;
        self.canonical_source_target_for_path(target, &path)
    }

    pub(crate) fn canonical_source_target_for_path(
        &self,
        target: &str,
        path: &str,
    ) -> Option<String> {
        let Some(fragment) = LinkTarget::parse(target).symbol() else {
            return Some(path.to_string());
        };
        self.source
            .graph()
            .canonical_symbol_target(&format!("{path}#{fragment}"))
    }

    fn source_files_matching_globs(&self, patterns: &[String]) -> Vec<String> {
        let matcher = GlobMatcher::from_valid_patterns(patterns);
        let mut files_by_pattern = vec![Vec::new(); patterns.len()];
        let mut indices = Vec::new();
        for source_file in self.source.paths() {
            matcher.matching_pattern_indices_into(source_file, &mut indices);
            for index in &indices {
                if let Some(files) = files_by_pattern.get_mut(*index) {
                    files.push(source_file.clone());
                }
            }
        }
        let mut resolved_files = Vec::new();
        for (index, pattern) in patterns.iter().enumerate() {
            let files = files_by_pattern.get(index).map_or(&[][..], Vec::as_slice);
            if files.is_empty()
                && let SourceTargetResolution::Resolved { path, .. } =
                    self.resolve_source_target(pattern)
            {
                resolved_files.push(path);
            } else {
                resolved_files.extend(files.iter().cloned());
            }
        }
        resolved_files
    }

    pub(crate) fn source_globs_have_matches(&self, patterns: &[String]) -> Vec<bool> {
        let matcher = GlobMatcher::from_valid_patterns(patterns);
        let mut matched_patterns = vec![false; patterns.len()];
        let mut indices = Vec::new();
        for source_file in self.source.paths() {
            matcher.matching_pattern_indices_into(source_file, &mut indices);
            for index in &indices {
                if let Some(value) = matched_patterns.get_mut(*index) {
                    *value = true;
                }
            }
        }
        for (index, pattern) in patterns.iter().enumerate() {
            if let Some(value) = matched_patterns.get_mut(index) {
                *value |= matches!(
                    self.resolve_source_target(pattern),
                    SourceTargetResolution::Resolved { .. }
                );
            }
        }
        matched_patterns
    }

    pub(crate) fn source_files(&self) -> &[String] {
        self.source.paths()
    }

    pub(crate) const fn source_graph(&self) -> &SourceGraph {
        self.source.graph()
    }

    pub(crate) const fn source_state(&self) -> &SourceState {
        &self.source
    }

    pub(crate) fn documentation_assets(&self) -> &[DocumentationAsset] {
        &self.assets
    }

    pub(crate) const fn patterns(&self) -> &BTreeSet<String> {
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
        let (adr_id, local_id) = split_pattern_id(pattern_id)?;
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
            .find(|policy| policy.id.as_deref().map(str::trim) == Some(local_id))?;
        Some((note, policy))
    }

    /// An accepted decision that no accepted successor supersedes, directly
    /// or through a chain.
    pub(crate) fn is_effective_decision(&self, note: &Note) -> bool {
        note.kind == NoteKind::Decision
            && note
                .id
                .as_ref()
                .is_some_and(|id| self.effective_decisions.contains(id))
    }

    pub(crate) fn is_historical_decision(&self, note: &Note) -> bool {
        note.kind == NoteKind::Decision
            && note.status == Some(DecisionStatus::Accepted)
            && !self.is_effective_decision(note)
    }
}

/// Inventories the documentation assets in lexical order (ADR-0131). An
/// unsupported, mismatched, or oversized file is skipped; the inventory stops
/// at the first verified asset that would exceed the total bound.
fn load_documentation_assets(
    files: &RepositoryFiles,
    mut paths: Vec<String>,
    max_asset_bytes: u64,
    max_total_bytes: u64,
) -> Result<Vec<DocumentationAsset>> {
    paths.sort();
    let mut total_bytes = 0_u64;
    let mut assets = Vec::new();
    for path in paths {
        let Some(contents) = files.read_bounded(Path::new(&path), max_asset_bytes)? else {
            continue;
        };
        let Some(expected_mime) = asset::mime_for_path(&path) else {
            continue;
        };
        if infer::get(&contents).is_none_or(|detected| detected.mime_type() != expected_mime) {
            continue;
        }
        let bytes = u64::try_from(contents.len()).map_err(|_| {
            crate::CrivError::new(format!("asset `{path}` exceeds the supported byte range"))
        })?;
        if total_bytes.saturating_add(bytes) > max_total_bytes {
            break;
        }
        total_bytes = total_bytes.saturating_add(bytes);
        assets.push(DocumentationAsset {
            path,
            mime: expected_mime.to_string(),
            bytes,
            hash: content_hash::of_bytes(&contents),
        });
    }
    Ok(assets)
}

fn registered_policy_patterns(
    notes: &[Note],
    effective_decisions: &BTreeSet<String>,
) -> BTreeSet<String> {
    notes
        .iter()
        .filter(|note| note.kind == NoteKind::Decision)
        .filter_map(|note| note.id.as_deref().map(|id| (id, note)))
        .filter(|(id, _)| effective_decisions.contains(*id))
        .flat_map(|(id, note)| {
            note.policy_patterns.iter().filter_map(move |pattern| {
                pattern
                    .id
                    .as_deref()
                    .filter(|local_id| !local_id.trim().is_empty())
                    .map(|local_id| pattern_id(id, local_id))
            })
        })
        .collect()
}

fn effective_accepted_decision_ids(
    notes: &[Note],
    decisions: &BTreeMap<String, usize>,
) -> BTreeSet<String> {
    let decision = |id: &str| decisions.get(id).and_then(|index| notes.get(*index));
    let accepted = decisions
        .keys()
        .filter(|id| decision(id).is_some_and(|note| note.status == Some(DecisionStatus::Accepted)))
        .cloned()
        .collect::<BTreeSet<_>>();

    let mut superseded = BTreeSet::new();
    let mut pending = accepted
        .iter()
        .filter_map(|id| decision(id))
        .flat_map(|note| note.supersedes.iter().cloned())
        .collect::<Vec<_>>();
    while let Some(id) = pending.pop() {
        if !superseded.insert(id.clone()) {
            continue;
        }
        if let Some(note) = decision(&id) {
            pending.extend(note.supersedes.iter().cloned());
        }
    }

    accepted.difference(&superseded).cloned().collect()
}

fn parse_note(files: &RepositoryFiles, path: &Path) -> Result<Note> {
    let contents = read_selected_text_from(files, path)?;
    let source: Arc<str> = Arc::from(contents.as_str());
    let frontmatter = split_frontmatter(&contents);
    let (yaml, frontmatter_start, body_start) = frontmatter.map_or(("", 0, 0), |frontmatter| {
        (
            frontmatter.yaml,
            frontmatter.yaml_start,
            frontmatter.body_start,
        )
    });
    // Positions are file-relative (ADR-0045). The frontmatter offset applies to
    // the parsed-note body only; the error branch keeps the whole file as the
    // body, so it needs no offset at all.
    let mut note = match parse_frontmatter(yaml) {
        Ok(note) => Note {
            body: contents.get(body_start..).unwrap_or_default().to_string(),
            ..note
        },
        Err(err) => Note {
            body: contents.clone(),
            frontmatter_error_location: err.offset.and_then(|offset| {
                let offset = frontmatter_start.saturating_add(offset);
                SourceLocation::new(source.clone(), offset..offset)
            }),
            frontmatter_error: Some(err.message),
            ..Note::default()
        },
    };
    note.path = path.to_path_buf();
    note.rel_path = strip_prefix(path, files.root());
    let (line_offset, body_start) = if note.frontmatter_error.is_some() {
        (0, 0)
    } else {
        (line_index(&contents, body_start), body_start)
    };
    note.wiki_links = find_wiki_links_with_lines(&note.body)
        .into_iter()
        .map(|(line, raw, span)| WikiLink {
            target: LinkTarget::parse(&raw).as_str().to_string(),
            raw,
            line: line.saturating_add(line_offset),
            location: SourceLocation::new(
                source.clone(),
                body_start.saturating_add(span.start)..body_start.saturating_add(span.end),
            ),
        })
        .collect();
    note.headings = parse_markdown_headings(&note.body)
        .into_iter()
        .map(|(level, text, line)| Heading {
            level,
            text,
            line: line.saturating_add(line_offset),
        })
        .collect();
    Ok(note)
}

/// The frontmatter fields of a note, without its location or body.
fn parse_frontmatter(frontmatter: &str) -> std::result::Result<Note, FrontmatterParseError> {
    let raw = if frontmatter.trim().is_empty() {
        RawFrontmatter::default()
    } else {
        serde_norway::from_str::<RawFrontmatter>(frontmatter).map_err(|error| {
            FrontmatterParseError {
                offset: error.location().map(|location| location.index()),
                message: format!("failed to parse YAML frontmatter: {error}"),
            }
        })?
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
                        language: pattern
                            .language
                            .as_deref()
                            .map(str::trim)
                            .filter(|language| !language.is_empty())
                            .map(PolicyLanguage::from),
                        pattern: pattern.pattern,
                        rule: pattern.rule,
                        message: pattern.message,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Note {
        id: raw.id,
        kind: NoteKind::parse(raw.kind.as_deref()),
        title: raw.title,
        status: raw.status.as_deref().map(DecisionStatus::parse),
        targets_symbols,
        targets_scope,
        target_pattern_refs,
        target_pattern_ids,
        policy_patterns,
        governs: raw.governs,
        supersedes: raw.supersedes,
        superseded_by: raw.superseded_by,
        ..Note::default()
    })
}

#[derive(Debug)]
struct FrontmatterParseError {
    message: String,
    offset: Option<usize>,
}

fn note_has_heading(note: &Note, heading: &str) -> bool {
    let normalized = heading.trim();
    note.headings.iter().any(|candidate| {
        candidate.text.eq_ignore_ascii_case(normalized) || slug(&candidate.text) == slug(normalized)
    })
}

/// The file line of the first frontmatter line containing `needle`, or of the
/// first frontmatter line; the opening `---` is line 1.
fn frontmatter_line(frontmatter: &str, needle: &str) -> usize {
    frontmatter.find(needle).map_or(2, |offset| {
        line_number(frontmatter, offset).saturating_add(1)
    })
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
        .map_or_else(
            || frontmatter_line(frontmatter, "patterns:"),
            |needle| frontmatter_line(frontmatter, needle),
        )
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

    #[test]
    fn documentation_assets_are_verified_bounded_and_separate_from_source() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(root.path().join("docs/assets")).unwrap();
        let png = b"\x89PNG\r\n\x1a\npreview";
        std::fs::write(root.path().join("docs/assets/b.png"), png).unwrap();
        std::fs::write(root.path().join("docs/assets/a.png"), png).unwrap();
        std::fs::write(root.path().join("docs/assets/wrong.jpg"), png).unwrap();
        let oversized = [png.as_slice(), png.as_slice()].concat();
        std::fs::write(root.path().join("docs/assets/0-oversized.png"), oversized).unwrap();
        std::fs::write(root.path().join("docs/assets/active.svg"), b"<svg></svg>").unwrap();
        let files = RepositoryFiles::open(root.path()).unwrap();

        let assets = load_documentation_assets(
            &files,
            vec![
                "docs/assets/wrong.jpg".into(),
                "docs/assets/0-oversized.png".into(),
                "docs/assets/b.png".into(),
                "docs/assets/a.png".into(),
            ],
            u64::try_from(png.len()).unwrap(),
            u64::try_from(png.len()).unwrap(),
        )
        .unwrap();

        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].path, "docs/assets/a.png");
        assert_eq!(assets[0].mime, "image/png");
        assert_eq!(assets[0].bytes, u64::try_from(png.len()).unwrap());
        assert_eq!(assets[0].hash.len(), 64);

        std::fs::remove_file(root.path().join("docs/assets/0-oversized.png")).unwrap();
        let vault = docs_only(root.path());
        assert_eq!(
            vault
                .documentation_assets()
                .iter()
                .map(|asset| asset.path.as_str())
                .collect::<Vec<_>>(),
            ["docs/assets/a.png", "docs/assets/b.png"]
        );
        assert!(vault.source_files().is_empty());
    }

    #[test]
    fn documentation_asset_total_bound_stops_at_the_first_overflow() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(root.path().join("docs/assets")).unwrap();
        let ten_byte_png = b"\x89PNG\r\n\x1a\nAA";
        let eight_byte_png = b"\x89PNG\r\n\x1a\n";
        for (path, contents) in [
            ("docs/assets/a.png", ten_byte_png.as_slice()),
            ("docs/assets/b.png", ten_byte_png.as_slice()),
            ("docs/assets/c.png", eight_byte_png.as_slice()),
        ] {
            std::fs::write(root.path().join(path), contents).unwrap();
        }
        let files = RepositoryFiles::open(root.path()).unwrap();

        let assets = load_documentation_assets(
            &files,
            vec![
                "docs/assets/c.png".into(),
                "docs/assets/b.png".into(),
                "docs/assets/a.png".into(),
            ],
            u64::try_from(ten_byte_png.len()).unwrap(),
            18,
        )
        .unwrap();

        assert_eq!(
            assets
                .iter()
                .map(|asset| asset.path.as_str())
                .collect::<Vec<_>>(),
            ["docs/assets/a.png"]
        );
    }

    #[test]
    fn documentation_asset_total_bound_counts_only_verified_assets() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(root.path().join("docs/assets")).unwrap();
        let png = b"\x89PNG\r\n\x1a\nAA";
        std::fs::write(root.path().join("docs/assets/a.png"), png).unwrap();
        std::fs::write(root.path().join("docs/assets/b.png"), b"not a png!").unwrap();
        std::fs::write(root.path().join("docs/assets/c.png"), png).unwrap();
        let files = RepositoryFiles::open(root.path()).unwrap();

        let assets = load_documentation_assets(
            &files,
            vec![
                "docs/assets/a.png".into(),
                "docs/assets/b.png".into(),
                "docs/assets/c.png".into(),
            ],
            10,
            20,
        )
        .unwrap();

        assert_eq!(
            assets
                .iter()
                .map(|asset| asset.path.as_str())
                .collect::<Vec<_>>(),
            ["docs/assets/a.png", "docs/assets/c.png"]
        );
    }

    #[test]
    fn documentation_assets_support_the_passive_format_set() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(root.path().join("docs")).unwrap();
        for (path, contents) in [
            ("docs/a.png", b"\x89PNG\r\n\x1a\npreview".as_slice()),
            (
                "docs/b.jpg",
                b"\xff\xd8\xff\xe0\x00\x10JFIF\x00preview".as_slice(),
            ),
            ("docs/c.gif", b"GIF89a\x01\x00\x01\x00preview".as_slice()),
            (
                "docs/d.webp",
                b"RIFF\x10\x00\x00\x00WEBPVP8 preview".as_slice(),
            ),
            ("docs/e.pdf", b"%PDF-1.7\npreview".as_slice()),
        ] {
            std::fs::write(root.path().join(path), contents).unwrap();
        }

        let vault = docs_only(root.path());
        assert_eq!(
            vault
                .documentation_assets()
                .iter()
                .map(|asset| (asset.path.as_str(), asset.mime.as_str()))
                .collect::<Vec<_>>(),
            [
                ("docs/a.png", "image/png"),
                ("docs/b.jpg", "image/jpeg"),
                ("docs/c.gif", "image/gif"),
                ("docs/d.webp", "image/webp"),
                ("docs/e.pdf", "application/pdf"),
            ]
        );
    }

    #[test]
    fn documentation_asset_hash_changes_for_same_size_content() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(root.path().join("docs")).unwrap();
        let path = root.path().join("docs/preview.png");
        std::fs::write(&path, b"\x89PNG\r\n\x1a\na").unwrap();
        let before = docs_only(root.path());
        let before_hash = before.documentation_assets()[0].hash.clone();

        std::fs::write(&path, b"\x89PNG\r\n\x1a\nb").unwrap();
        let after = docs_only(root.path());

        assert_ne!(before_hash, after.documentation_assets()[0].hash);
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
        )
        .unwrap();

        assert_eq!(note.policy_patterns.len(), 2);
        assert_eq!(note.policy_patterns[0].id.as_deref(), Some("no-println"));
        assert_eq!(
            note.policy_patterns[0].language,
            Some(PolicyLanguage::Supported(
                ast_grep_language::SupportLang::Rust
            ))
        );
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
        let accepted = r#"---
id: ADR-0001
kind: decision
title: Accepted
status: accepted
policy:
  patterns:
    - id: accepted
---

# Accepted
"#;
        let draft = r#"---
id: ADR-0002
kind: decision
title: Draft
status: draft
policy:
  patterns:
    - id: draft
---

# Draft
"#;
        let missing_status = r#"---
id: ADR-0003
kind: decision
title: Missing status
policy:
  patterns:
    - id: missing-status
---

# Missing status
"#;
        let non_decision = r#"---
id: DOC-0001
kind: doc
title: Documentation
status: accepted
policy:
  patterns:
    - id: documentation
---

# Documentation
"#;
        let (_root, vault) = notes_vault(&[accepted, draft, missing_status, non_decision]);

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
    fn accepted_successors_make_predecessors_historical_transitively() {
        let old = r#"---
id: ADR-0001
kind: decision
title: Old
status: accepted
policy:
  patterns:
    - id: old
---

# Old
"#;
        let middle = r#"---
id: ADR-0002
kind: decision
title: Middle
status: draft
supersedes:
  - ADR-0001
---

# Middle
"#;
        let newest = r#"---
id: ADR-0003
kind: decision
title: Newest
status: accepted
supersedes:
  - ADR-0002
---

# Newest
"#;
        let (_root, vault) = notes_vault(&[old, middle, newest]);

        assert!(vault.is_historical_decision(&vault.notes()[0]));
        assert!(vault.patterns().is_empty());
        assert!(vault.resolve_policy_pattern("ADR-0001/old").is_some());

        let draft_successor = r#"---
id: ADR-0002
kind: decision
title: Draft successor
status: draft
supersedes:
  - ADR-0001
---

# Draft successor
"#;
        let (_root, vault) = notes_vault(&[old, draft_successor]);
        assert!(vault.is_effective_decision(&vault.notes()[0]));
        assert_eq!(
            vault.patterns(),
            &BTreeSet::from(["ADR-0001/old".to_string()])
        );
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
        let root = tempfile::TempDir::new().unwrap();
        let root = root.path();
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

        work::reset();
        let vault = load(root);
        assert_eq!(work::count(Work::LinkSourceResolution), 2);

        work::reset();
        for link in vault.notes().iter().flat_map(Note::wiki_links) {
            vault.resolve_link(&link.target);
        }
        assert!(matches!(
            vault.resolve_link("collision-note"),
            ResolvedLink::Note { .. }
        ));
        assert!(matches!(
            vault.resolve_link("ADR-0001#match:ADR-0001/no-block-on"),
            ResolvedLink::Pattern { .. }
        ));
        assert_eq!(work::count(Work::LinkSourceResolution), 0);

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
    }

    #[test]
    fn disabled_source_index_skips_source_collection_and_graph() {
        let root = tempfile::TempDir::new().unwrap();
        let root = root.path();
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

        let vault = load(root);

        assert!(vault.source_files().is_empty());
        assert!(vault.source_graph().files().next().is_none());
    }

    #[test]
    fn vault_uses_the_observed_source_state() {
        let root = tempfile::TempDir::new().unwrap();
        let root = root.path();
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

        let config = Config::load(root).unwrap();
        let files = RepositoryFiles::open(root).unwrap();
        let source = SourceState::refresh(&files, &config, None).unwrap();
        let expected = source.paths().to_vec();
        let vault = Vault::load(
            &files,
            LoadOptions {
                source: SourceLoad::Observed(source),
                ..LoadOptions::default()
            },
        )
        .unwrap();

        assert_eq!(vault.source_files(), expected);
        assert!(vault.source_graph().file("src/lib.rs").is_some());
        work::reset();
        assert_eq!(
            vault.resolve_source_target("src/lib.rs#run"),
            SourceTargetResolution::Resolved {
                path: "src/lib.rs".into(),
                ambiguous: false,
            }
        );
        assert_eq!(work::count(Work::SourceTargetResolution), 1);
    }

    #[test]
    fn batch_source_globs_preserve_order_fallbacks_and_invalid_patterns() {
        let root = tempfile::TempDir::new().unwrap();
        let root = root.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(
            root.join("criv.toml"),
            "[source]\nroots = [\"src\", \"assets/blob.bin\"]\n",
        )
        .unwrap();
        std::fs::write(root.join("src/a.rs"), "pub fn a() {}\n").unwrap();
        std::fs::write(root.join("src/z.rs"), "pub fn run() {}\n").unwrap();
        std::fs::write(
            root.join("assets/blob.bin"),
            "text with a binary extension\n",
        )
        .unwrap();

        let vault = load(root);
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
    }

    /// Returns the 1-based line of the first line whose content equals `needle`.
    fn real_line(contents: &str, needle: &str) -> usize {
        contents
            .lines()
            .position(|line| line.trim_end() == needle)
            .expect("needle must appear in the fixture")
            .saturating_add(1)
    }

    fn load(root: &Path) -> Vault {
        Vault::load(
            &RepositoryFiles::open(root).unwrap(),
            LoadOptions::default(),
        )
        .unwrap()
    }

    fn docs_only(root: &Path) -> Vault {
        Vault::load(
            &RepositoryFiles::open(root).unwrap(),
            LoadOptions {
                source: SourceLoad::Skip,
                ..LoadOptions::default()
            },
        )
        .unwrap()
    }

    /// A documents-only vault with one `docs/note-N.md` per entry.
    fn notes_vault(notes: &[&str]) -> (tempfile::TempDir, Vault) {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(root.path().join("docs")).unwrap();
        for (index, contents) in notes.iter().enumerate() {
            std::fs::write(root.path().join(format!("docs/note-{index}.md")), contents).unwrap();
        }
        let vault = docs_only(root.path());
        (root, vault)
    }

    fn parsed_note(contents: &str) -> Note {
        let (_root, vault) = notes_vault(&[contents]);
        vault.notes()[0].clone()
    }

    #[test]
    fn note_positions_are_file_relative_with_frontmatter() {
        let contents =
            "---\nid: DOC-1\nkind: doc\ntitle: Doc\n---\n\n# Heading\n\nSee [[other-note]].\n";
        let note = parsed_note(contents);

        assert_eq!(note.headings[0].line, real_line(contents, "# Heading"));
        assert_eq!(
            note.wiki_links[0].line,
            real_line(contents, "See [[other-note]].")
        );
        let exact = note.wiki_links[0]
            .location
            .as_ref()
            .expect("parsed wiki-link has an exact location")
            .lsp_range()
            .expect("the validated source location has an LSP range");
        assert_eq!((exact.start.line, exact.start.character), (8, 4));
        assert_eq!((exact.end.line, exact.end.character), (8, 18));
    }

    #[test]
    fn note_positions_are_file_relative_without_frontmatter() {
        let contents = "# Heading\n\nSee [[other-note]].\n";
        let note = parsed_note(contents);

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
        let note = parsed_note(contents);

        assert!(note.frontmatter_error.is_some());
        assert!(note.frontmatter_error_location.is_some());
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
}
