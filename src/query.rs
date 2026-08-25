use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use usage::{Args as UsageArgs, Subcommands, ValueEnum};

use crate::repository::RepositoryFiles;
use crate::source::SymbolKind;
use crate::vault::{
    NoteKind, ResolvedLink, SourceTargetResolution, Vault, source_fragment_name,
    source_fragment_path,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryCapability {
    Snapshot,
    Docs,
    Sources,
}

impl QueryCommand {
    const fn capability(&self) -> QueryCapability {
        match self {
            Self::Diff(_) => QueryCapability::Snapshot,
            Self::NextAdrId(_) | Self::CitedBy(_) | Self::OrphanDocs(_) => QueryCapability::Docs,
            Self::Nodes(options)
                if matches!(options.kind, Some(NodeKind::Doc | NodeKind::Decision)) =>
            {
                QueryCapability::Docs
            }
            Self::Callers(_)
            | Self::Callees(_)
            | Self::AttackSurface(_)
            | Self::Targets(_)
            | Self::Cites(_)
            | Self::References(_)
            | Self::Governs(_)
            | Self::Governing(_)
            | Self::Coverage(_)
            | Self::Nodes(_) => QueryCapability::Sources,
        }
    }

    const fn reverse_index_scope(&self) -> Option<ReverseIndexScope> {
        match self {
            Self::CitedBy(_) | Self::OrphanDocs(_) => Some(ReverseIndexScope::Notes),
            Self::References(_)
            | Self::Nodes(NodesOptions {
                kind: Some(NodeKind::Code),
                without_docs: true,
                ..
            }) => Some(ReverseIndexScope::Sources),
            _ => None,
        }
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
            vec![false; vault.notes.len()]
        } else {
            Vec::new()
        };
        let mut source_references = BTreeMap::<SourceReferenceKey, Vec<usize>>::new();

        for (note_index, note) in vault.notes.iter().enumerate() {
            let mut cited_note_ids = BTreeSet::new();
            let mut source_keys = BTreeSet::new();

            for link in &note.wiki_links {
                if include_notes && let ResolvedLink::Note { id } = vault.resolve_link(&link.target)
                {
                    notes_with_outgoing_citations[note_index] = true;
                    if id != note.display_id() {
                        cited_note_ids.insert(id);
                    }
                }
                if include_sources {
                    collect_source_reference_keys(vault, &link.target, &mut source_keys);
                }
            }
            if include_sources {
                for target in &note.targets_symbols {
                    collect_source_reference_keys(vault, target, &mut source_keys);
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
        let source_path = source_fragment_path(symbol);
        let requested_fragment = source_fragment_name(symbol);
        let Some((path, _)) = vault.resolve_source_path(source_path) else {
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
    target: &str,
    keys: &mut BTreeSet<SourceReferenceKey>,
) {
    let SourceTargetResolution::Resolved { path, .. } = vault.resolve_source_target(target) else {
        return;
    };
    keys.insert(SourceReferenceKey::Path(path.clone()));
    if let Some(fragment) = source_fragment_name(target) {
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
        .map(|index| vault.notes[*index].display_id().to_string())
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
    RepositoryFiles::open_vault(root)?;
    if let QueryCommand::Diff(options) = &options.command {
        let rows = diff(root, &options.ref_a, &options.ref_b)?;
        return print_rows(&rows, options.output);
    }

    let vault = load_query_vault(root, &options.command)?;
    let reverse_index = options
        .command
        .reverse_index_scope()
        .map(|scope| QueryReverseIndex::build(&vault, scope));
    let (rows, output) = match options.command {
        QueryCommand::NextAdrId(output) => (vec![next_adr_id(&vault)?], output),
        QueryCommand::Callers(options) => {
            let rows = vault.source_graph().callers(&options.symbol);
            (rows, options.output)
        }
        QueryCommand::Callees(options) => {
            let rows = vault.source_graph().callees(&options.symbol);
            (rows, options.output)
        }
        QueryCommand::AttackSurface(output) => (vault.source_graph().attack_surface(), output),
        QueryCommand::Targets(options) => {
            let rows = targets(&vault, &options.note_id)?;
            (rows, options.output)
        }
        QueryCommand::Cites(options) => {
            let rows = cites(&vault, &options.note_id, false)?;
            (rows, options.output)
        }
        QueryCommand::CitedBy(options) => {
            let rows = cited_by(
                &vault,
                required_reverse_index(reverse_index.as_ref())?,
                &options.note_id,
            )?;
            (rows, options.output)
        }
        QueryCommand::OrphanDocs(output) => (
            orphan_docs(&vault, required_reverse_index(reverse_index.as_ref())?),
            output,
        ),
        QueryCommand::References(options) => {
            let rows =
                required_reverse_index(reverse_index.as_ref())?.references(&vault, &options.symbol);
            (rows, options.output)
        }
        QueryCommand::Governs(options) => {
            let rows = governs(&vault, &options.adr_id)?;
            (rows, options.output)
        }
        QueryCommand::Governing(options) => {
            let rows = governing(&vault, &options.symbol);
            (rows, options.output)
        }
        QueryCommand::Coverage(options) => {
            let rows = coverage(&vault, options.by);
            (rows, options.output)
        }
        QueryCommand::Nodes(options) => {
            let rows = nodes(
                &vault,
                reverse_index.as_ref(),
                options.kind,
                options.without_docs,
            )?;
            (rows, options.output)
        }
        QueryCommand::Diff(_) => {
            return Err(CrivError::new("snapshot query reached vault loading"));
        }
    };

    print_rows(&rows, output)
}

fn load_query_vault(root: &Path, command: &QueryCommand) -> Result<Vault> {
    match command.capability() {
        QueryCapability::Docs => Vault::load_docs_only(root),
        QueryCapability::Sources => Vault::load(root),
        QueryCapability::Snapshot => Err(CrivError::new("snapshot query does not load a vault")),
    }
}

const MAX_ADR_NUMBER: u32 = 9999;

fn next_adr_id(vault: &Vault) -> Result<String> {
    let highest = vault
        .notes
        .iter()
        .filter_map(|note| note.id.as_deref())
        .filter_map(|id| id.strip_prefix("ADR-"))
        .filter_map(|digits| digits.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    let next = highest
        .checked_add(1)
        .filter(|next| *next <= MAX_ADR_NUMBER);
    match next {
        Some(next) => Ok(format!("ADR-{next:04}")),
        None => Err(CrivError::coded_fix(
            "adr-id-exhausted",
            format!("no free ADR id after ADR-{highest:04}; ids are four digits"),
            "Retire or renumber the highest ADR ids, or widen the id format in a new ADR.",
        )),
    }
}

fn targets(vault: &Vault, id: &str) -> Result<Vec<String>> {
    let note = vault
        .resolve_note(id)
        .ok_or_else(|| CrivError::new(format!("note `{id}` does not resolve")))?;
    let mut rows = note.targets_symbols.clone();
    for link in &note.wiki_links {
        match vault.resolve_link(&link.target) {
            ResolvedLink::Source { path, .. } => rows.push(path),
            ResolvedLink::Pattern { id } => rows.push(format!("match:{id}")),
            _ => {}
        }
    }
    rows.sort();
    rows.dedup();
    Ok(rows)
}

fn cites(vault: &Vault, id: &str, note_only: bool) -> Result<Vec<String>> {
    let note = vault
        .resolve_note(id)
        .ok_or_else(|| CrivError::new(format!("note `{id}` does not resolve")))?;
    let mut rows = Vec::new();
    for link in &note.wiki_links {
        match vault.resolve_link(&link.target) {
            ResolvedLink::Note { id } => rows.push(id),
            ResolvedLink::Source { path, .. } if !note_only => rows.push(path),
            ResolvedLink::Pattern { id } if !note_only => rows.push(format!("match:{id}")),
            _ => {}
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
    for (note_index, note) in vault.notes.iter().enumerate() {
        if note.kind != NoteKind::Doc {
            continue;
        }
        let id = note.display_id();
        let has_outgoing = index.notes_with_outgoing_citations[note_index];
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
    let mut rows = vault.source_files_matching_globs(&vault.effective_governs(note));
    rows.sort();
    rows.dedup();
    Ok(rows)
}

fn governing(vault: &Vault, symbol: &str) -> Vec<String> {
    let Some((path, _)) = vault.resolve_source_path(source_fragment_path(symbol)) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for note in &vault.notes {
        if note.kind != NoteKind::Decision {
            continue;
        }
        if vault
            .source_files_matching_globs(&vault.effective_governs(note))
            .contains(&path)
        {
            rows.push(note.display_id().to_string());
        }
    }
    rows.sort();
    rows
}

fn coverage(vault: &Vault, by: Option<CoverageBy>) -> Vec<String> {
    let governed = vault
        .notes
        .iter()
        .filter(|note| note.kind == NoteKind::Decision)
        .flat_map(|note| vault.source_files_matching_globs(&vault.effective_governs(note)))
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
        entry.0 += 1;
        if governed.contains(source_file) {
            entry.1 += 1;
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
    for note in &vault.notes {
        if note.kind != NoteKind::Decision {
            continue;
        }
        let governed = vault
            .source_files_matching_globs(&vault.effective_governs(note))
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

fn required_reverse_index(index: Option<&QueryReverseIndex>) -> Result<&QueryReverseIndex> {
    index.ok_or_else(|| CrivError::new("query command requires a reverse index"))
}

fn nodes(
    vault: &Vault,
    reverse_index: Option<&QueryReverseIndex>,
    kind: Option<NodeKind>,
    without_docs: bool,
) -> Result<Vec<String>> {
    let mut rows = Vec::new();
    match kind {
        Some(NodeKind::Code) => {
            for symbol in vault.source_graph().symbols() {
                let display = symbol.id.display();
                if without_docs
                    && !required_reverse_index(reverse_index)?
                        .references(vault, &display)
                        .is_empty()
                {
                    continue;
                }
                rows.push(display);
            }
        }
        Some(NodeKind::Doc) => rows.extend(
            vault
                .notes
                .iter()
                .filter(|note| note.kind == NoteKind::Doc)
                .map(|note| note.display_id().to_string()),
        ),
        Some(NodeKind::Decision) => rows.extend(
            vault
                .notes
                .iter()
                .filter(|note| note.kind == NoteKind::Decision)
                .map(|note| note.display_id().to_string()),
        ),
        Some(kind) => {
            let symbol_kind = kind
                .symbol_kind()
                .ok_or_else(|| CrivError::new("node kind does not identify a source symbol"))?;
            rows.extend(
                vault
                    .source_graph()
                    .symbols()
                    .filter(|symbol| symbol.kind == symbol_kind)
                    .map(|symbol| symbol.id.display()),
            );
        }
        _ => {
            rows.extend(vault.source_files().iter().cloned());
            rows.extend(vault.notes.iter().map(|note| note.display_id().to_string()));
        }
    }
    rows.sort();
    Ok(rows)
}

fn diff(root: &Path, left: &str, right: &str) -> Result<Vec<String>> {
    let left = load_snapshot(root, left)?;
    let right = load_snapshot(root, right)?;
    let left_nodes = json_string_set(&left, "/graph/nodes", "id");
    let right_nodes = json_string_set(&right, "/graph/nodes", "id");
    let left_edges = json_edge_set(&left);
    let right_edges = json_edge_set(&right);

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

fn load_snapshot(root: &Path, id: &str) -> Result<serde_json::Value> {
    let local = if id == "latest" || is_snapshot_hash(id) {
        crate::state::load_snapshot(root, id)?
    } else {
        None
    };
    let contents = if let Some(contents) = local {
        contents
    } else {
        load_git_state(root, id)?
    };
    serde_json::from_str(&contents)
        .map_err(|err| CrivError::new(format!("failed to parse snapshot `{id}`: {err}")))
}

fn is_snapshot_hash(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn load_git_state(root: &Path, id: &str) -> Result<String> {
    let bytes =
        crate::git::read_file_at_ref(root, id, Path::new(".criv/state.json")).map_err(|error| {
            CrivError::new(format!(
                "snapshot or git ref `{id}` does not resolve: {error}"
            ))
        })?;
    String::from_utf8(bytes).map_err(|err| {
        CrivError::new(format!(
            "git ref `{id}` produced non-UTF-8 .criv/state.json: {err}"
        ))
    })
}

fn json_string_set(value: &serde_json::Value, pointer: &str, field: &str) -> BTreeSet<String> {
    value
        .pointer(pointer)
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get(field).and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect()
}

fn json_edge_set(value: &serde_json::Value) -> BTreeSet<String> {
    value
        .pointer("/graph/edges")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            Some(format!(
                "{}:{}:{}",
                item.get("from")?.as_str()?,
                item.get("kind")?.as_str()?,
                item.get("to")?.as_str()?
            ))
        })
        .collect()
}

fn print_rows(rows: &[String], output: OutputOptions) -> Result<()> {
    let rows = match output.limit {
        Some(limit) => &rows[..rows.len().min(limit)],
        None => rows,
    };
    match output.format {
        Format::Text => {
            for row in rows {
                println!("{row}");
            }
            Ok(())
        }
        Format::Json => {
            let json = serde_json::to_string_pretty(rows)
                .map_err(|err| CrivError::new(format!("failed to serialize query rows: {err}")))?;
            println!("{json}");
            Ok(())
        }
        Format::Ndjson => {
            for row in rows {
                let json = serde_json::to_string(row).map_err(|err| {
                    CrivError::new(format!("failed to serialize query row: {err}"))
                })?;
                println!("{json}");
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use tempfile::TempDir;

    #[test]
    fn every_query_variant_declares_its_data_capability() {
        let cases = vec![
            (
                QueryCommand::NextAdrId(query_output_options()),
                QueryCapability::Docs,
            ),
            (
                QueryCommand::Callers(query_symbol_options()),
                QueryCapability::Sources,
            ),
            (
                QueryCommand::Callees(query_symbol_options()),
                QueryCapability::Sources,
            ),
            (
                QueryCommand::AttackSurface(query_output_options()),
                QueryCapability::Sources,
            ),
            (
                QueryCommand::Targets(query_note_options()),
                QueryCapability::Sources,
            ),
            (
                QueryCommand::Cites(query_note_options()),
                QueryCapability::Sources,
            ),
            (
                QueryCommand::CitedBy(query_note_options()),
                QueryCapability::Docs,
            ),
            (
                QueryCommand::OrphanDocs(query_output_options()),
                QueryCapability::Docs,
            ),
            (
                QueryCommand::References(query_symbol_options()),
                QueryCapability::Sources,
            ),
            (
                QueryCommand::Governs(query_decision_options()),
                QueryCapability::Sources,
            ),
            (
                QueryCommand::Governing(query_symbol_options()),
                QueryCapability::Sources,
            ),
            (
                QueryCommand::Coverage(CoverageOptions {
                    by: None,
                    output: query_output_options(),
                }),
                QueryCapability::Sources,
            ),
            (
                query_nodes_command(Some(NodeKind::Doc), false),
                QueryCapability::Docs,
            ),
            (
                query_nodes_command(Some(NodeKind::Doc), true),
                QueryCapability::Docs,
            ),
            (
                query_nodes_command(Some(NodeKind::Decision), false),
                QueryCapability::Docs,
            ),
            (
                query_nodes_command(Some(NodeKind::Code), false),
                QueryCapability::Sources,
            ),
            (
                query_nodes_command(Some(NodeKind::Module), false),
                QueryCapability::Sources,
            ),
            (query_nodes_command(None, false), QueryCapability::Sources),
            (
                QueryCommand::Diff(DiffOptions {
                    ref_a: "latest".into(),
                    ref_b: "latest".into(),
                    output: query_output_options(),
                }),
                QueryCapability::Snapshot,
            ),
        ];

        for (command, expected) in cases {
            assert_eq!(command.capability(), expected, "{command:?}");
        }
    }

    #[test]
    fn source_node_kinds_map_to_native_symbol_kinds() {
        let cases = [
            (NodeKind::Function, SymbolKind::Function),
            (NodeKind::Method, SymbolKind::Method),
            (NodeKind::Class, SymbolKind::Class),
            (NodeKind::Module, SymbolKind::Module),
            (NodeKind::Protocol, SymbolKind::Protocol),
            (NodeKind::Implementation, SymbolKind::Implementation),
            (NodeKind::Struct, SymbolKind::Struct),
            (NodeKind::Exception, SymbolKind::Exception),
            (NodeKind::Behaviour, SymbolKind::Behaviour),
            (NodeKind::Macro, SymbolKind::Macro),
            (NodeKind::Guard, SymbolKind::Guard),
            (NodeKind::Callback, SymbolKind::Callback),
            (NodeKind::MacroCallback, SymbolKind::MacroCallback),
        ];

        for (node_kind, symbol_kind) in cases {
            assert_eq!(node_kind.symbol_kind(), Some(symbol_kind));
        }
        for node_kind in [NodeKind::Code, NodeKind::Doc, NodeKind::Decision] {
            assert_eq!(node_kind.symbol_kind(), None);
        }
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

        crate::source::reset_index_work_counts();
        crate::source::reset_graph_work_counts();
        for command in &commands {
            let vault = load_query_vault(temp.path(), command).unwrap();
            assert!(vault.source_files().is_empty());
            assert!(vault.source_graph().files.is_empty());
        }

        let source_index = crate::source::index_work_counts();
        assert_eq!(source_index.discovery_scans, 0);
        let source_graph = crate::source::graph_work_counts();
        assert_eq!(source_graph.cache_loads, 0);
        assert_eq!(source_graph.parsed_files, 0);
        assert_eq!(source_graph.reused_files, 0);
        assert_eq!(source_graph.cache_publications, 0);
        assert!(!temp.path().join(".criv/source-graph.json").exists());
    }

    #[test]
    fn source_query_loading_retains_full_vault_work() {
        let temp = TempDir::new().unwrap();
        write_query_fixture(temp.path());
        let command = QueryCommand::AttackSurface(query_output_options());

        crate::source::reset_index_work_counts();
        crate::source::reset_graph_work_counts();
        let vault = load_query_vault(temp.path(), &command).unwrap();

        assert_eq!(vault.source_files().len(), 2);
        assert_eq!(crate::source::index_work_counts().discovery_scans, 1);
        let source_graph = crate::source::graph_work_counts();
        assert_eq!(source_graph.cache_loads, 1);
        assert_eq!(source_graph.parsed_files, 2);
        assert_eq!(source_graph.cache_publications, 1);
        assert!(temp.path().join(".criv/source-graph.json").exists());
    }

    #[test]
    fn source_reverse_index_bounds_work_and_stored_rows() {
        let temp = TempDir::new().unwrap();
        write_reverse_query_fixture(temp.path());
        let vault = Vault::load(temp.path()).unwrap();

        crate::vault::reset_work_counts();
        let reverse_index = QueryReverseIndex::build(&vault, ReverseIndexScope::Sources);
        assert_eq!(
            nodes(&vault, Some(&reverse_index), Some(NodeKind::Code), true).unwrap(),
            vec!["src/lib.rs#fn:undocumented"]
        );
        let input_targets = vault
            .notes
            .iter()
            .map(|note| note.targets_symbols.len() + note.wiki_links.len())
            .sum::<usize>();
        let stored_rows = reverse_index
            .source_references
            .values()
            .map(Vec::len)
            .sum::<usize>();
        assert_eq!(
            crate::vault::work_counts().source_target_resolutions(),
            input_targets
        );
        assert!(stored_rows <= input_targets * 2);
        assert!(reverse_index.citing_notes.is_empty());
        assert!(reverse_index.notes_with_outgoing_citations.is_empty());
    }

    #[test]
    fn note_reverse_index_bounds_stored_rows() {
        let temp = TempDir::new().unwrap();
        write_reverse_query_fixture(temp.path());
        let vault = Vault::load_docs_only(temp.path()).unwrap();

        let reverse_index = QueryReverseIndex::build(&vault, ReverseIndexScope::Notes);
        let input_links = vault
            .notes
            .iter()
            .map(|note| note.wiki_links.len())
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

    #[test]
    fn snapshot_hash_shape() {
        assert!(is_snapshot_hash("abc123"));
        assert!(!is_snapshot_hash("../../etc/passwd"));
        assert!(!is_snapshot_hash("HEAD~1"));
        assert!(!is_snapshot_hash(""));
    }

    fn query_output_options() -> OutputOptions {
        OutputOptions {
            format: Format::Text,
            limit: None,
        }
    }

    fn query_symbol_options() -> SymbolOptions {
        SymbolOptions {
            symbol: "src/lib.rs#fn:run".into(),
            output: query_output_options(),
        }
    }

    fn query_note_options() -> NoteOptions {
        NoteOptions {
            note_id: "c4".into(),
            output: query_output_options(),
        }
    }

    fn query_decision_options() -> DecisionOptions {
        DecisionOptions {
            adr_id: "ADR-0001".into(),
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
