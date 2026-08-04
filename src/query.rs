use std::collections::BTreeSet;
use std::path::Path;

use clap::{Args as ClapArgs, Subcommand, ValueEnum};

use crate::c4_code;
use crate::vault::{
    NoteKind, ResolvedLink, SourceTargetResolution, Vault, source_fragment_name,
    source_fragment_path,
};
use crate::{CrivError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
}

#[derive(Debug, ClapArgs)]
pub(crate) struct QueryOptions {
    #[command(subcommand)]
    command: QueryCommand,
}

#[derive(Debug, Subcommand)]
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
    /// Emit focused LikeC4 source for modules in a source path glob.
    C4Code(PathGlobOptions),
    /// Compare two state snapshots or git refs.
    Diff(DiffOptions),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryCapability {
    Snapshot,
    Docs,
    Sources,
}

impl QueryCommand {
    fn capability(&self) -> QueryCapability {
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
            | Self::Nodes(_)
            | Self::C4Code(_) => QueryCapability::Sources,
        }
    }
}

#[derive(Debug, ClapArgs)]
struct OutputOptions {
    /// Select text rows or a JSON array of rows.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
}

#[derive(Debug, ClapArgs)]
struct SymbolOptions {
    /// Source path or symbol selector.
    symbol: String,
    #[command(flatten)]
    output: OutputOptions,
}

#[derive(Debug, ClapArgs)]
struct NoteOptions {
    /// Note id or unique note name.
    note_id: String,
    #[command(flatten)]
    output: OutputOptions,
}

#[derive(Debug, ClapArgs)]
struct DecisionOptions {
    /// ADR id.
    adr_id: String,
    #[command(flatten)]
    output: OutputOptions,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CoverageBy {
    Module,
    Adr,
}

#[derive(Debug, ClapArgs)]
struct CoverageOptions {
    /// Group coverage rows by module or ADR.
    #[arg(long, value_enum)]
    by: Option<CoverageBy>,
    #[command(flatten)]
    output: OutputOptions,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum NodeKind {
    Code,
    Doc,
    Decision,
}

#[derive(Debug, ClapArgs)]
struct NodesOptions {
    /// Restrict nodes to code, documentation, or decisions.
    #[arg(long, value_enum)]
    kind: Option<NodeKind>,
    /// Restrict code nodes to symbols that no note references.
    #[arg(long)]
    without_docs: bool,
    #[command(flatten)]
    output: OutputOptions,
}

#[derive(Debug, ClapArgs)]
struct PathGlobOptions {
    /// Source path or component/module glob.
    path_glob: String,
    #[command(flatten)]
    output: OutputOptions,
}

#[derive(Debug, ClapArgs)]
struct DiffOptions {
    /// Left snapshot hash, `latest`, or git ref.
    ref_a: String,
    /// Right snapshot hash, `latest`, or git ref.
    ref_b: String,
    #[command(flatten)]
    output: OutputOptions,
}

pub(crate) fn run(root: &Path, options: QueryOptions) -> Result<()> {
    if let QueryCommand::Diff(options) = &options.command {
        let rows = diff(root, &options.ref_a, &options.ref_b)?;
        return print_rows(&rows, options.output.format);
    }

    let vault = load_query_vault(root, &options.command)?;
    let (rows, format) = match options.command {
        QueryCommand::NextAdrId(output) => (vec![next_adr_id(&vault)], output.format),
        QueryCommand::Callers(options) => {
            let rows = vault.source_graph().callers(&options.symbol);
            (rows, options.output.format)
        }
        QueryCommand::Callees(options) => {
            let rows = vault.source_graph().callees(&options.symbol);
            (rows, options.output.format)
        }
        QueryCommand::AttackSurface(output) => {
            (vault.source_graph().attack_surface(), output.format)
        }
        QueryCommand::Targets(options) => {
            let rows = targets(&vault, &options.note_id)?;
            (rows, options.output.format)
        }
        QueryCommand::Cites(options) => {
            let rows = cites(&vault, &options.note_id, false)?;
            (rows, options.output.format)
        }
        QueryCommand::CitedBy(options) => {
            let rows = cited_by(&vault, &options.note_id)?;
            (rows, options.output.format)
        }
        QueryCommand::OrphanDocs(output) => (orphan_docs(&vault), output.format),
        QueryCommand::References(options) => {
            let rows = references(&vault, &options.symbol);
            (rows, options.output.format)
        }
        QueryCommand::Governs(options) => {
            let rows = governs(&vault, &options.adr_id)?;
            (rows, options.output.format)
        }
        QueryCommand::Governing(options) => {
            let rows = governing(&vault, &options.symbol);
            (rows, options.output.format)
        }
        QueryCommand::Coverage(options) => {
            let rows = coverage(&vault, options.by);
            (rows, options.output.format)
        }
        QueryCommand::Nodes(options) => {
            let rows = nodes(&vault, options.kind, options.without_docs);
            (rows, options.output.format)
        }
        QueryCommand::C4Code(options) => (
            c4_code::for_glob(&vault, &options.path_glob),
            options.output.format,
        ),
        QueryCommand::Diff(_) => unreachable!("snapshot queries return before vault loading"),
    };

    print_rows(&rows, format)
}

fn load_query_vault(root: &Path, command: &QueryCommand) -> Result<Vault> {
    match command.capability() {
        QueryCapability::Docs => Vault::load_docs_only(root),
        QueryCapability::Sources => Vault::load(root),
        QueryCapability::Snapshot => {
            unreachable!("snapshot queries do not construct a vault")
        }
    }
}

fn next_adr_id(vault: &Vault) -> String {
    let next = vault
        .notes
        .iter()
        .filter_map(|note| note.id.as_deref())
        .filter_map(|id| id.strip_prefix("ADR-"))
        .filter_map(|digits| digits.parse::<u32>().ok())
        .max()
        .unwrap_or(0)
        + 1;
    format!("ADR-{next:04}")
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

fn cited_by(vault: &Vault, id: &str) -> Result<Vec<String>> {
    let target = vault
        .resolve_note(id)
        .ok_or_else(|| CrivError::new(format!("note `{id}` does not resolve")))?;
    let target_id = target.display_id();
    let mut rows = Vec::new();

    for note in &vault.notes {
        if note.display_id() == target_id {
            continue;
        }
        for link in &note.wiki_links {
            if let ResolvedLink::Note { id } = vault.resolve_link(&link.target)
                && id == target_id
            {
                rows.push(note.display_id().to_string());
                break;
            }
        }
    }

    rows.sort();
    Ok(rows)
}

fn orphan_docs(vault: &Vault) -> Vec<String> {
    let mut rows = Vec::new();
    for note in &vault.notes {
        if note.kind != NoteKind::Doc {
            continue;
        }
        let id = note.display_id();
        let outgoing = cites(vault, id, true).unwrap_or_default();
        let incoming = cited_by(vault, id).unwrap_or_default();
        if outgoing.is_empty() && incoming.is_empty() {
            rows.push(id.to_string());
        }
    }
    rows.sort();
    rows
}

fn references(vault: &Vault, symbol: &str) -> Vec<String> {
    let source_path = source_fragment_path(symbol);
    let requested_fragment = source_fragment_name(symbol);
    let Some((path, _)) = vault.resolve_source_path(source_path) else {
        return Vec::new();
    };

    let mut rows = Vec::new();
    for note in &vault.notes {
        let frontmatter_refs = note
            .targets_symbols
            .iter()
            .any(|target| target_matches_source(vault, target, &path, requested_fragment));
        let body_refs = note
            .wiki_links
            .iter()
            .any(|link| target_matches_source(vault, &link.target, &path, requested_fragment));
        if frontmatter_refs || body_refs {
            rows.push(note.display_id().to_string());
        }
    }
    rows.sort();
    rows.dedup();
    rows
}

fn target_matches_source(
    vault: &Vault,
    target: &str,
    resolved_path: &str,
    requested_fragment: Option<&str>,
) -> bool {
    let SourceTargetResolution::Resolved { path, .. } = vault.resolve_source_target(target) else {
        return false;
    };
    if path != resolved_path {
        return false;
    }
    match requested_fragment {
        Some(fragment) => source_fragment_name(target) == Some(fragment),
        None => true,
    }
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
            .map(|(parent, _)| parent)
            .unwrap_or(".")
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

fn nodes(vault: &Vault, kind: Option<NodeKind>, without_docs: bool) -> Vec<String> {
    let mut rows = Vec::new();
    match kind {
        Some(NodeKind::Code) => {
            for symbol in vault.source_graph().symbols() {
                let display = symbol.id.display();
                if without_docs && !references(vault, &display).is_empty() {
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
        _ => {
            rows.extend(vault.source_files().iter().cloned());
            rows.extend(vault.notes.iter().map(|note| note.display_id().to_string()));
        }
    }
    rows.sort();
    rows
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
        crate::snapshots::load(root, id)?
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

fn print_rows(rows: &[String], format: Format) -> Result<()> {
    match format {
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
            (query_nodes_command(None, false), QueryCapability::Sources),
            (
                QueryCommand::C4Code(PathGlobOptions {
                    path_glob: "src/**".into(),
                    output: query_output_options(),
                }),
                QueryCapability::Sources,
            ),
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

        crate::source_index::reset_work_counts();
        crate::source_graph::reset_work_counts();
        for command in &commands {
            let vault = load_query_vault(temp.path(), command).unwrap();
            assert!(vault.source_files().is_empty());
            assert!(vault.source_graph().files.is_empty());
        }

        let source_index = crate::source_index::work_counts();
        assert_eq!(source_index.catalog_traversals, 0);
        assert_eq!(source_index.source_enumerations, 0);
        let source_graph = crate::source_graph::work_counts();
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

        crate::source_index::reset_work_counts();
        crate::source_graph::reset_work_counts();
        let vault = load_query_vault(temp.path(), &command).unwrap();

        assert_eq!(vault.source_files().len(), 2);
        assert_eq!(crate::source_index::work_counts().catalog_traversals, 1);
        assert_eq!(crate::source_index::work_counts().source_enumerations, 1);
        let source_graph = crate::source_graph::work_counts();
        assert_eq!(source_graph.cache_loads, 1);
        assert_eq!(source_graph.parsed_files, 2);
        assert_eq!(source_graph.cache_publications, 1);
        assert!(temp.path().join(".criv/source-graph.json").exists());
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
}
