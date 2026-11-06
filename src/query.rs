use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use criv_state_wire::{GraphRowsView, layout};
use usage_rs::{Args as UsageArgs, Subcommands, ValueEnum};

use crate::diagnostic::DiagnosticCode;
use crate::identity::{MAX_ADR_NUMBER, format_adr_id, parse_adr_id};
use crate::markdown::LinkTarget;
use crate::repository::RepositoryFiles;
use crate::source::SymbolKind;
use crate::state::SnapshotId;
use crate::vault::{
    LoadOptions, NoteKind, ResolvedLink, SourceLoad, SourceTargetResolution, Vault,
};
use crate::{CrivError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
    Ndjson,
}

/// Ask the loaded vault graph a focused question.
///
/// Each query prints one row per result. Add `--format json` to any query for a
/// JSON array of rows, as in `criv query nodes --kind decision --format json`.
#[derive(Debug, UsageArgs)]
pub struct QueryOptions {
    #[usage(subcommand)]
    command: QueryCommand,
}

#[derive(Debug, Subcommands)]
enum QueryCommand {
    /// Print the next ADR id after the highest existing ADR id.
    NextAdrId(OutputOptions),
    /// List source symbols that call the requested symbol.
    Callers(SymbolOptions),
    /// List source symbols called by the requested symbol.
    Callees(SymbolOptions),
    /// List exported or public source symbols in the source graph.
    AttackSurface(OutputOptions),
    /// List source and pattern targets declared or linked by a note.
    Targets(NoteOptions),
    /// List notes, sources, and patterns cited by a note.
    Cites(NoteOptions),
    /// List notes that cite the requested note.
    CitedBy(NoteOptions),
    /// List documentation notes without incoming or outgoing note citations.
    OrphanDocs(OutputOptions),
    /// List notes that reference a source path or symbol.
    References(SymbolOptions),
    /// List source files governed by a decision.
    Governs(DecisionOptions),
    /// List decisions that govern a source path or symbol.
    Governing(SymbolOptions),
    /// Summarize source governance coverage.
    Coverage(CoverageOptions),
    /// List source, note, or decision nodes.
    Nodes(NodesOptions),
    /// Compare two state snapshots or git refs.
    ///
    /// `diff` resolves `latest` through `.criv/latest`, hex-like values through
    /// `.criv/snapshots/<hash>.json`, and any other value through an embedded
    /// lookup of `.criv/state.json` in the requested repository ref. It does
    /// not invoke the `git` executable.
    Diff(DiffOptions),
}

impl QueryCommand {
    /// Loads only the data this command reads, then answers it.
    fn answer(self, files: &RepositoryFiles) -> Result<(Vec<String>, OutputOptions)> {
        Ok(match self {
            Self::Diff(options) => (diff(files, &options.ref_a, &options.ref_b)?, options.output),
            Self::NextAdrId(output) => (
                vec![next_adr_id(&load_vault(files, SourceLoad::Skip)?)?],
                output,
            ),
            Self::Callers(options) => (
                load_vault(files, SourceLoad::Refresh)?
                    .source_graph()
                    .callers(&options.symbol),
                options.output,
            ),
            Self::Callees(options) => (
                load_vault(files, SourceLoad::Refresh)?
                    .source_graph()
                    .callees(&options.symbol),
                options.output,
            ),
            Self::AttackSurface(output) => (
                load_vault(files, SourceLoad::Refresh)?
                    .source_graph()
                    .attack_surface(),
                output,
            ),
            Self::Targets(options) => (
                targets(&load_vault(files, SourceLoad::Refresh)?, &options.note_id)?,
                options.output,
            ),
            Self::Cites(options) => (
                cites(&load_vault(files, SourceLoad::Refresh)?, &options.note_id)?,
                options.output,
            ),
            Self::CitedBy(options) => {
                let vault = load_vault(files, SourceLoad::Skip)?;
                let index = QueryReverseIndex::build(&vault, ReverseIndexScope::Notes);
                (cited_by(&vault, &index, &options.note_id)?, options.output)
            }
            Self::OrphanDocs(output) => {
                let vault = load_vault(files, SourceLoad::Skip)?;
                let index = QueryReverseIndex::build(&vault, ReverseIndexScope::Notes);
                (orphan_docs(&vault, &index), output)
            }
            Self::References(options) => {
                let vault = load_vault(files, SourceLoad::Refresh)?;
                let index = QueryReverseIndex::build(&vault, ReverseIndexScope::Sources);
                (index.references(&vault, &options.symbol), options.output)
            }
            Self::Governs(options) => (
                governs(&load_vault(files, SourceLoad::Refresh)?, &options.adr_id)?,
                options.output,
            ),
            Self::Governing(options) => (
                governing(&load_vault(files, SourceLoad::Refresh)?, &options.symbol),
                options.output,
            ),
            Self::Coverage(options) => (
                coverage(&load_vault(files, SourceLoad::Refresh)?, options.by),
                options.output,
            ),
            Self::Nodes(options) => (nodes(files, &options)?, options.output),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReverseIndexScope {
    Notes,
    Sources,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum SourceReferenceKey {
    Path(String),
    Symbol { path: String, fragment: String },
}

#[derive(Debug)]
struct QueryReverseIndex {
    citing_notes: BTreeMap<String, Vec<usize>>,
    notes_with_outgoing_citations: Vec<bool>,
    source_references: BTreeMap<SourceReferenceKey, Vec<usize>>,
}

impl QueryReverseIndex {
    fn build(vault: &Vault, scope: ReverseIndexScope) -> Self {
        let include_notes = scope == ReverseIndexScope::Notes;
        let include_sources = scope == ReverseIndexScope::Sources;
        let mut citing_notes = BTreeMap::<String, Vec<usize>>::new();
        let mut notes_with_outgoing_citations = if include_notes {
            vec![false; vault.notes().len()]
        } else {
            Vec::new()
        };
        let mut source_references = BTreeMap::<SourceReferenceKey, Vec<usize>>::new();

        for (note_index, note) in vault.notes().iter().enumerate() {
            let mut cited_note_ids = BTreeSet::new();
            let mut source_keys = BTreeSet::new();

            for link in note.wiki_links() {
                if include_notes
                    && let ResolvedLink::Note { id, .. } = vault.resolve_link(link.target())
                {
                    if let Some(has_outgoing) = notes_with_outgoing_citations.get_mut(note_index) {
                        *has_outgoing = true;
                    }
                    if id != note.display_id() {
                        cited_note_ids.insert(id);
                    }
                }
                if include_sources {
                    collect_source_reference_keys(vault, link.target(), &mut source_keys);
                }
            }
            if include_sources {
                for target in note.targets_symbols() {
                    collect_source_reference_keys(
                        vault,
                        LinkTarget::parse(target),
                        &mut source_keys,
                    );
                }
            }

            for cited_id in cited_note_ids {
                citing_notes.entry(cited_id).or_default().push(note_index);
            }
            for key in source_keys {
                source_references.entry(key).or_default().push(note_index);
            }
        }

        Self {
            citing_notes,
            notes_with_outgoing_citations,
            source_references,
        }
    }

    fn citing_notes(&self, vault: &Vault, note_id: &str) -> Vec<String> {
        sorted_note_ids(vault, self.citing_notes.get(note_id).map(Vec::as_slice))
    }

    fn references(&self, vault: &Vault, symbol: &str) -> Vec<String> {
        let link = LinkTarget::parse(symbol);
        let requested_fragment = link.symbol();
        let Some((path, _)) = vault.resolve_source_path(link) else {
            return Vec::new();
        };
        let key = match requested_fragment {
            Some(fragment) => SourceReferenceKey::Symbol {
                path,
                fragment: fragment.to_string(),
            },
            None => SourceReferenceKey::Path(path),
        };
        sorted_note_ids(vault, self.source_references.get(&key).map(Vec::as_slice))
    }
}

fn collect_source_reference_keys(
    vault: &Vault,
    target: LinkTarget<'_>,
    keys: &mut BTreeSet<SourceReferenceKey>,
) {
    let SourceTargetResolution::Resolved { path, .. } = vault.resolve_source_target(target) else {
        return;
    };
    keys.insert(SourceReferenceKey::Path(path.clone()));
    if let Some(fragment) = target.symbol() {
        keys.insert(SourceReferenceKey::Symbol {
            path,
            fragment: fragment.to_string(),
        });
    }
}

fn sorted_note_ids(vault: &Vault, note_indexes: Option<&[usize]>) -> Vec<String> {
    let mut rows = note_indexes
        .into_iter()
        .flatten()
        .filter_map(|index| vault.notes().get(*index))
        .map(|note| note.display_id().to_string())
        .collect::<Vec<_>>();
    rows.sort();
    rows
}

#[derive(Debug, Clone, Copy, UsageArgs)]
struct OutputOptions {
    /// Select text rows, a JSON array of rows, or one JSON row per line.
    #[usage(long, value_enum, default = "text")]
    format: Format,
    /// Print at most this many rows, so a large graph cannot flood a caller.
    #[usage(long)]
    limit: Option<usize>,
}

#[derive(Debug, UsageArgs)]
struct SymbolOptions {
    /// Source path or symbol selector.
    symbol: String,
    #[usage(flatten)]
    output: OutputOptions,
}

#[derive(Debug, UsageArgs)]
struct NoteOptions {
    /// Note id or unique note name.
    note_id: String,
    #[usage(flatten)]
    output: OutputOptions,
}

#[derive(Debug, UsageArgs)]
struct DecisionOptions {
    /// ADR id.
    adr_id: String,
    #[usage(flatten)]
    output: OutputOptions,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CoverageBy {
    Module,
    Adr,
}

#[derive(Debug, UsageArgs)]
struct CoverageOptions {
    /// Group coverage rows by module or ADR.
    #[usage(long, value_enum)]
    by: Option<CoverageBy>,
    #[usage(flatten)]
    output: OutputOptions,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum NodeKind {
    Code,
    Doc,
    Decision,
    Function,
    Method,
    Class,
    Module,
    Protocol,
    Implementation,
    Struct,
    Exception,
    Behaviour,
    Macro,
    Guard,
    Callback,
    MacroCallback,
}

impl NodeKind {
    const fn symbol_kind(self) -> Option<SymbolKind> {
        match self {
            Self::Function => Some(SymbolKind::Function),
            Self::Method => Some(SymbolKind::Method),
            Self::Class => Some(SymbolKind::Class),
            Self::Module => Some(SymbolKind::Module),
            Self::Protocol => Some(SymbolKind::Protocol),
            Self::Implementation => Some(SymbolKind::Implementation),
            Self::Struct => Some(SymbolKind::Struct),
            Self::Exception => Some(SymbolKind::Exception),
            Self::Behaviour => Some(SymbolKind::Behaviour),
            Self::Macro => Some(SymbolKind::Macro),
            Self::Guard => Some(SymbolKind::Guard),
            Self::Callback => Some(SymbolKind::Callback),
            Self::MacroCallback => Some(SymbolKind::MacroCallback),
            Self::Code | Self::Doc | Self::Decision => None,
        }
    }
}

#[derive(Debug, UsageArgs)]
struct NodesOptions {
    /// Restrict nodes to code, a source symbol kind, documentation, or decisions.
    #[usage(long, value_enum)]
    kind: Option<NodeKind>,
    /// Restrict code nodes to symbols that no note references.
    #[usage(long)]
    without_docs: bool,
    #[usage(flatten)]
    output: OutputOptions,
}

#[derive(Debug, UsageArgs)]
struct DiffOptions {
    /// Left snapshot hash, `latest`, or git ref.
    ref_a: String,
    /// Right snapshot hash, `latest`, or git ref.
    ref_b: String,
    #[usage(flatten)]
    output: OutputOptions,
}

pub fn run(root: &Path, options: QueryOptions) -> Result<()> {
    let files = crate::config::open_vault(root)?;
    let (rows, output) = options.command.answer(&files)?;
    print_rows(&rows, output)
}

fn load_vault(files: &RepositoryFiles, source: SourceLoad) -> Result<Vault> {
    Vault::load(
        files,
        LoadOptions {
            source,
            ..LoadOptions::default()
        },
    )
}

fn next_adr_id(vault: &Vault) -> Result<String> {
    let highest = vault
        .notes()
        .iter()
        .filter_map(|note| note.id())
        .filter_map(parse_adr_id)
        .max()
        .unwrap_or(0);
    let next = highest
        .checked_add(1)
        .filter(|next| *next <= MAX_ADR_NUMBER);
    next.map_or_else(
        || {
            Err(CrivError::coded(
                DiagnosticCode::AdrIdExhausted,
                format!(
                    "no free ADR id after {}; ids are four digits",
                    format_adr_id(highest)
                ),
            ))
        },
        |next| Ok(format_adr_id(next)),
    )
}

fn targets(vault: &Vault, id: &str) -> Result<Vec<String>> {
    let note = vault
        .resolve_note(id)
        .ok_or_else(|| CrivError::new(format!("note `{id}` does not resolve")))?;
    let mut rows = note.targets_symbols().to_vec();
    for link in note.wiki_links() {
        match vault.resolve_link(link.target()) {
            ResolvedLink::Source { path, .. } => rows.push(path),
            ResolvedLink::Pattern { id } => rows.push(format!("match:{id}")),
            _ => {}
        }
    }
    rows.sort();
    rows.dedup();
    Ok(rows)
}

fn cites(vault: &Vault, id: &str) -> Result<Vec<String>> {
    let note = vault
        .resolve_note(id)
        .ok_or_else(|| CrivError::new(format!("note `{id}` does not resolve")))?;
    let mut rows = Vec::new();
    for link in note.wiki_links() {
        match vault.resolve_link(link.target()) {
            ResolvedLink::Note { id, .. } => rows.push(id),
            ResolvedLink::Source { path, .. } => rows.push(path),
            ResolvedLink::Pattern { id } => rows.push(format!("match:{id}")),
            ResolvedLink::Broken => {}
        }
    }
    rows.sort();
    rows.dedup();
    Ok(rows)
}

fn cited_by(vault: &Vault, index: &QueryReverseIndex, id: &str) -> Result<Vec<String>> {
    let target = vault
        .resolve_note(id)
        .ok_or_else(|| CrivError::new(format!("note `{id}` does not resolve")))?;
    let target_id = target.display_id();
    Ok(index.citing_notes(vault, target_id))
}

fn orphan_docs(vault: &Vault, index: &QueryReverseIndex) -> Vec<String> {
    let mut rows = Vec::new();
    for (note_index, note) in vault.notes().iter().enumerate() {
        if note.kind() != NoteKind::Doc {
            continue;
        }
        let id = note.display_id();
        let has_outgoing = index
            .notes_with_outgoing_citations
            .get(note_index)
            .copied()
            .unwrap_or(false);
        let has_incoming = index.citing_notes.contains_key(id);
        if !has_outgoing && !has_incoming {
            rows.push(id.to_string());
        }
    }
    rows.sort();
    rows
}

fn governs(vault: &Vault, adr_id: &str) -> Result<Vec<String>> {
    let note = vault
        .resolve_note(adr_id)
        .ok_or_else(|| CrivError::new(format!("decision `{adr_id}` does not resolve")))?;
    let mut rows = vault.governed_files(note);
    rows.sort();
    rows.dedup();
    Ok(rows)
}

fn governing(vault: &Vault, symbol: &str) -> Vec<String> {
    let Some((path, _)) = vault.resolve_source_path(LinkTarget::parse(symbol)) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for note in vault.notes() {
        if note.kind() != NoteKind::Decision {
            continue;
        }
        if vault.governed_files(note).contains(&path) {
            rows.push(note.display_id().to_string());
        }
    }
    rows.sort();
    rows
}

fn coverage(vault: &Vault, by: Option<CoverageBy>) -> Vec<String> {
    let governed = vault
        .notes()
        .iter()
        .filter(|note| note.kind() == NoteKind::Decision)
        .flat_map(|note| vault.governed_files(note))
        .collect::<std::collections::BTreeSet<_>>();
    match by {
        Some(CoverageBy::Module) => return coverage_by_module(vault, &governed),
        Some(CoverageBy::Adr) => return coverage_by_adr(vault),
        None => {}
    }
    vec![
        format!("source_files={}", vault.source_files().len()),
        format!("governed_files={}", governed.len()),
        format!(
            "ungoverned_files={}",
            vault.source_files().len().saturating_sub(governed.len())
        ),
    ]
}

fn coverage_by_module(vault: &Vault, governed: &BTreeSet<String>) -> Vec<String> {
    let mut modules = std::collections::BTreeMap::<String, (usize, usize)>::new();
    for source_file in vault.source_files() {
        let module = source_file
            .rsplit_once('/')
            .map_or(".", |(parent, _)| parent)
            .to_string();
        let entry = modules.entry(module).or_default();
        entry.0 = entry.0.saturating_add(1);
        if governed.contains(source_file) {
            entry.1 = entry.1.saturating_add(1);
        }
    }
    modules
        .into_iter()
        .map(|(module, (total, governed))| {
            format!(
                "module={module} source_files={total} governed_files={governed} ungoverned_files={}",
                total.saturating_sub(governed)
            )
        })
        .collect()
}

fn coverage_by_adr(vault: &Vault) -> Vec<String> {
    let mut rows = Vec::new();
    for note in vault.notes() {
        if note.kind() != NoteKind::Decision {
            continue;
        }
        let governed = vault
            .governed_files(note)
            .into_iter()
            .collect::<BTreeSet<_>>();
        rows.push(format!(
            "adr={} governed_files={}",
            note.display_id(),
            governed.len()
        ));
    }
    rows.sort();
    rows
}

fn nodes(files: &RepositoryFiles, options: &NodesOptions) -> Result<Vec<String>> {
    let mut rows = match options.kind {
        Some(NodeKind::Doc) => note_ids(&load_vault(files, SourceLoad::Skip)?, NoteKind::Doc),
        Some(NodeKind::Decision) => {
            note_ids(&load_vault(files, SourceLoad::Skip)?, NoteKind::Decision)
        }
        Some(NodeKind::Code) => {
            let vault = load_vault(files, SourceLoad::Refresh)?;
            let index = options
                .without_docs
                .then(|| QueryReverseIndex::build(&vault, ReverseIndexScope::Sources));
            code_nodes(&vault, index.as_ref())
        }
        Some(kind) => {
            let symbol_kind = kind
                .symbol_kind()
                .ok_or_else(|| CrivError::new("node kind does not identify a source symbol"))?;
            load_vault(files, SourceLoad::Refresh)?
                .source_graph()
                .symbols()
                .filter(|symbol| symbol.kind == symbol_kind)
                .map(|symbol| symbol.id.display())
                .collect()
        }
        None => {
            let vault = load_vault(files, SourceLoad::Refresh)?;
            let mut rows = vault.source_files().to_vec();
            rows.extend(
                vault
                    .notes()
                    .iter()
                    .map(|note| note.display_id().to_string()),
            );
            rows
        }
    };
    rows.sort();
    Ok(rows)
}

fn note_ids(vault: &Vault, kind: NoteKind) -> Vec<String> {
    vault
        .notes()
        .iter()
        .filter(|note| note.kind() == kind)
        .map(|note| note.display_id().to_string())
        .collect()
}

/// Every source symbol, or with `undocumented` only those no note references.
fn code_nodes(vault: &Vault, undocumented: Option<&QueryReverseIndex>) -> Vec<String> {
    vault
        .source_graph()
        .symbols()
        .map(|symbol| symbol.id.display())
        .filter(|display| {
            undocumented.is_none_or(|index| index.references(vault, display).is_empty())
        })
        .collect()
}

fn diff(files: &RepositoryFiles, left: &str, right: &str) -> Result<Vec<String>> {
    let left = load_snapshot(files, left)?;
    let right = load_snapshot(files, right)?;
    let left_nodes = left.node_ids();
    let right_nodes = right.node_ids();
    let left_edges = left.edge_rows();
    let right_edges = right.edge_rows();

    let mut rows = Vec::new();
    rows.extend(
        right_nodes
            .difference(&left_nodes)
            .map(|value| format!("node_added {value}")),
    );
    rows.extend(
        left_nodes
            .difference(&right_nodes)
            .map(|value| format!("node_removed {value}")),
    );
    rows.extend(
        right_edges
            .difference(&left_edges)
            .map(|value| format!("edge_added {value}")),
    );
    rows.extend(
        left_edges
            .difference(&right_edges)
            .map(|value| format!("edge_removed {value}")),
    );
    rows.sort();
    Ok(rows)
}

fn load_snapshot(files: &RepositoryFiles, id: &str) -> Result<GraphRowsView> {
    let local = match id.parse::<SnapshotId>() {
        Ok(snapshot) => crate::state::load_snapshot(files, &snapshot)?,
        Err(()) => None,
    };
    let contents = if let Some(contents) = local {
        contents
    } else {
        load_git_state(files.root(), id)?
    };
    serde_json::from_str(&contents)
        .map_err(|err| CrivError::new(format!("failed to parse snapshot `{id}`: {err}")))
}

fn load_git_state(root: &Path, id: &str) -> Result<String> {
    let file = crate::git::GitRepository::open(root)
        .and_then(|repository| {
            repository.file(&crate::git::Revision::Commit(id.to_owned()), layout::STATE)
        })
        .and_then(|file| {
            file.ok_or_else(|| {
                CrivError::new(format!(
                    "git ref `{id}` does not contain `{}`",
                    layout::STATE
                ))
            })
        })
        .map_err(|error| {
            CrivError::new(format!(
                "snapshot or git ref `{id}` does not resolve: {error}"
            ))
        })?;
    String::from_utf8(file.bytes).map_err(|err| {
        CrivError::new(format!(
            "git ref `{id}` produced non-UTF-8 {}: {err}",
            layout::STATE
        ))
    })
}

fn print_rows(rows: &[String], output: OutputOptions) -> Result<()> {
    let rows = output.limit.map_or(rows, |limit| {
        rows.get(..rows.len().min(limit)).unwrap_or(rows)
    });
    match output.format {
        Format::Text => {
            for row in rows {
                println!("{row}");
            }
        }
        Format::Json => {
            let json = serde_json::to_string_pretty(rows)
                .map_err(|err| CrivError::new(format!("failed to serialize query rows: {err}")))?;
            println!("{json}");
        }
        Format::Ndjson => {
            for row in rows {
                let json = serde_json::to_string(row).map_err(|err| {
                    CrivError::new(format!("failed to serialize query row: {err}"))
                })?;
                println!("{json}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::work::{self, Work};
    use tempfile::TempDir;

    fn files(root: &Path) -> RepositoryFiles {
        RepositoryFiles::open(root).unwrap()
    }

    #[test]
    fn docs_query_loading_performs_no_source_work() {
        let temp = TempDir::new().unwrap();
        write_query_fixture(temp.path());
        let commands = [
            QueryCommand::NextAdrId(query_output_options()),
            QueryCommand::CitedBy(query_note_options()),
            QueryCommand::OrphanDocs(query_output_options()),
            query_nodes_command(Some(NodeKind::Doc), false),
            query_nodes_command(Some(NodeKind::Decision), false),
        ];

        work::reset();
        for command in commands {
            let cited_by = matches!(command, QueryCommand::CitedBy(_));
            let answer = command.answer(&files(temp.path()));
            assert_eq!(answer.is_err(), cited_by, "{answer:?}");
        }

        assert_eq!(
            work::counts([
                Work::DiscoveryScan,
                Work::GraphCacheLoad,
                Work::SourceParse,
                Work::SourceReuse,
                Work::GraphCachePublication,
            ]),
            [0; 5]
        );
        assert!(!temp.path().join(".criv/source-graph.json").exists());
    }

    #[test]
    fn source_query_loading_retains_full_vault_work() {
        let temp = TempDir::new().unwrap();
        write_query_fixture(temp.path());
        let command = QueryCommand::AttackSurface(query_output_options());

        work::reset();
        command.answer(&files(temp.path())).unwrap();

        assert_eq!(
            work::counts([
                Work::DiscoveryScan,
                Work::GraphCacheLoad,
                Work::SourceParse,
                Work::GraphCachePublication,
            ]),
            [1, 1, 2, 1]
        );
        assert!(temp.path().join(".criv/source-graph.json").exists());
    }

    #[test]
    fn source_reverse_index_bounds_work_and_stored_rows() {
        let temp = TempDir::new().unwrap();
        write_reverse_query_fixture(temp.path());
        let vault = load_vault(&files(temp.path()), SourceLoad::Refresh).unwrap();

        work::reset();
        let reverse_index = QueryReverseIndex::build(&vault, ReverseIndexScope::Sources);
        assert_eq!(
            code_nodes(&vault, Some(&reverse_index)),
            vec!["src/lib.rs#fn:undocumented"]
        );
        let input_targets = vault
            .notes()
            .iter()
            .map(|note| note.targets_symbols().len() + note.wiki_links().len())
            .sum::<usize>();
        let stored_rows = reverse_index
            .source_references
            .values()
            .map(Vec::len)
            .sum::<usize>();
        assert_eq!(work::count(Work::SourceTargetResolution), input_targets);
        assert!(stored_rows <= input_targets * 2);
        assert!(reverse_index.citing_notes.is_empty());
        assert!(reverse_index.notes_with_outgoing_citations.is_empty());
    }

    #[test]
    fn note_reverse_index_bounds_stored_rows() {
        let temp = TempDir::new().unwrap();
        write_reverse_query_fixture(temp.path());
        let vault = load_vault(&files(temp.path()), SourceLoad::Skip).unwrap();

        let reverse_index = QueryReverseIndex::build(&vault, ReverseIndexScope::Notes);
        let input_links = vault
            .notes()
            .iter()
            .map(|note| note.wiki_links().len())
            .sum::<usize>();
        let stored_rows = reverse_index
            .citing_notes
            .values()
            .map(Vec::len)
            .sum::<usize>();

        assert!(stored_rows <= input_links);
        assert_eq!(
            reverse_index.citing_notes(&vault, "ADR-0001"),
            vec!["guide"]
        );
        assert!(reverse_index.citing_notes(&vault, "guide").is_empty());
        assert_eq!(
            reverse_index
                .notes_with_outgoing_citations
                .iter()
                .filter(|value| **value)
                .count(),
            1
        );
        assert!(reverse_index.source_references.is_empty());
    }

    fn query_output_options() -> OutputOptions {
        OutputOptions {
            format: Format::Text,
            limit: None,
        }
    }

    fn query_note_options() -> NoteOptions {
        NoteOptions {
            note_id: "c4".into(),
            output: query_output_options(),
        }
    }

    fn query_nodes_command(kind: Option<NodeKind>, without_docs: bool) -> QueryCommand {
        QueryCommand::Nodes(NodesOptions {
            kind,
            without_docs,
            output: query_output_options(),
        })
    }

    fn write_query_fixture(root: &Path) {
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("other")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(
            root.join("criv.toml"),
            r#"
[source]
roots = ["src", "other"]
"#,
        )
        .unwrap();
        fs::write(
            root.join("src/lib.rs"),
            r#"
struct Foo;

impl Foo {
    fn run(&self) {
        helper();
        external();
    }
}

fn helper() {}
"#,
        )
        .unwrap();
        fs::write(root.join("other/out.rs"), "fn external() {}\n").unwrap();
    }

    fn write_reverse_query_fixture(root: &Path) {
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("docs/adr")).unwrap();
        fs::write(
            root.join("criv.toml"),
            r#"
[vault]
docs = "docs"
adr = "adr"

[source]
roots = ["src"]
exclude = []
"#,
        )
        .unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn run() {}\npub fn helper() {}\npub fn undocumented() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("docs/guide.md"),
            r#"---
id: guide
kind: doc
title: Guide
targets:
  symbols:
    - src/lib.rs#fn:run
---

# Guide

See [[ADR-0001]], [[src/lib.rs#fn:helper]], and [[guide]].
"#,
        )
        .unwrap();
        fs::write(
            root.join("docs/adr/0001-test.md"),
            "---\nid: ADR-0001\nkind: decision\ntitle: Test\nstatus: accepted\n---\n",
        )
        .unwrap();
    }
}
