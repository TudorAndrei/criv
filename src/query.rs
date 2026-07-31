use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use clap::{Args as ClapArgs, ValueEnum};

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
    name: String,
    #[arg()]
    values: Vec<String>,
    #[arg(long)]
    by: Option<String>,
    #[arg(long)]
    kind: Option<String>,
    #[arg(long)]
    without_docs: bool,
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
}

pub(crate) fn run(root: &Path, options: QueryOptions) -> Result<()> {
    let vault = Vault::load(root)?;
    let rows = match options.name.as_str() {
        "next-adr-id" => vec![next_adr_id(&vault)],
        "callers" => {
            let symbol = required_arg(&options, "symbol")?;
            vault.source_graph().callers(symbol)
        }
        "callees" => {
            let symbol = required_arg(&options, "symbol")?;
            vault.source_graph().callees(symbol)
        }
        "attack-surface" => vault.source_graph().attack_surface(),
        "targets" => {
            let id = required_arg(&options, "note-id")?;
            targets(&vault, id)?
        }
        "cites" => {
            let id = required_arg(&options, "note-id")?;
            cites(&vault, id, false)?
        }
        "cited-by" => {
            let id = required_arg(&options, "note-id")?;
            cited_by(&vault, id)?
        }
        "orphan-docs" => orphan_docs(&vault),
        "references" => {
            let symbol = required_arg(&options, "symbol")?;
            references(&vault, symbol)
        }
        "governs" => {
            let adr_id = required_arg(&options, "ADR-ID")?;
            governs(&vault, adr_id)?
        }
        "governing" => {
            let symbol = required_arg(&options, "symbol")?;
            governing(&vault, symbol)
        }
        "coverage" => coverage(&vault, options.by.as_deref()),
        "nodes" => nodes(&vault, options.kind.as_deref(), options.without_docs),
        "c4-elements" => {
            let id = required_arg(&options, "note-id")?;
            c4_elements(&vault, id)?
        }
        "c4-relationships" => {
            let id = required_arg(&options, "note-id")?;
            c4_relationships(&vault, id)?
        }
        "c4-code" => {
            let glob = required_arg(&options, "path-glob")?;
            c4_code::for_glob(&vault, glob)
        }
        "diff" => {
            let left = required_arg(&options, "ref-a")?;
            let right = options
                .values
                .get(1)
                .map(String::as_str)
                .ok_or_else(|| CrivError::usage("query `diff` requires <ref-a> <ref-b>"))?;
            diff(root, left, right)?
        }
        other => {
            return Err(CrivError::usage(format!(
                "query `{other}` is not implemented in this MVP"
            )));
        }
    };

    print_rows(&rows, options.format)
}

fn required_arg<'a>(options: &'a QueryOptions, name: &str) -> Result<&'a str> {
    options
        .values
        .first()
        .map(String::as_str)
        .ok_or_else(|| CrivError::usage(format!("query `{}` requires <{name}>", options.name)))
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
    let mut rows = Vec::new();
    for pattern in vault.effective_governs(note) {
        rows.extend(vault.source_files_matching_glob(&pattern));
    }
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
            .effective_governs(note)
            .iter()
            .any(|pattern| vault.source_files_matching_glob(pattern).contains(&path))
        {
            rows.push(note.display_id().to_string());
        }
    }
    rows.sort();
    rows
}

fn coverage(vault: &Vault, by: Option<&str>) -> Vec<String> {
    let governed = vault
        .notes
        .iter()
        .filter(|note| note.kind == NoteKind::Decision)
        .flat_map(|note| {
            vault
                .effective_governs(note)
                .into_iter()
                .flat_map(|pattern| vault.source_files_matching_glob(&pattern))
        })
        .collect::<std::collections::BTreeSet<_>>();
    if by == Some("module") {
        return coverage_by_module(vault, &governed);
    }
    if by == Some("adr") {
        return coverage_by_adr(vault);
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
            .effective_governs(note)
            .into_iter()
            .flat_map(|pattern| vault.source_files_matching_glob(&pattern))
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

fn nodes(vault: &Vault, kind: Option<&str>, without_docs: bool) -> Vec<String> {
    let mut rows = Vec::new();
    match kind {
        Some("code") => {
            for symbol in vault.source_graph().symbols() {
                let display = symbol.id.display();
                if without_docs && !references(vault, &display).is_empty() {
                    continue;
                }
                rows.push(display);
            }
        }
        Some("doc") => rows.extend(
            vault
                .notes
                .iter()
                .filter(|note| note.kind == NoteKind::Doc)
                .map(|note| note.display_id().to_string()),
        ),
        Some("decision") => rows.extend(
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

fn c4_elements(vault: &Vault, id: &str) -> Result<Vec<String>> {
    let note = vault
        .resolve_note(id)
        .ok_or_else(|| CrivError::new(format!("note `{id}` does not resolve")))?;
    let mut rows = Vec::new();
    for diagram in &note.c4_diagrams {
        for element in &diagram.elements {
            let source = match &element.source {
                None => "none".to_string(),
                Some(source) => match vault.resolve_source_target(source) {
                    SourceTargetResolution::Resolved { path, .. } => path,
                    SourceTargetResolution::MissingFile
                    | SourceTargetResolution::MissingFragment { .. } => "unresolved".into(),
                },
            };
            rows.push(format!(
                "level={} alias={} category={} kind={} source={}",
                diagram.level.as_str(),
                element.alias,
                element.category.as_str(),
                element.kind,
                source
            ));
        }
    }
    Ok(rows)
}

fn c4_relationships(vault: &Vault, id: &str) -> Result<Vec<String>> {
    let note = vault
        .resolve_note(id)
        .ok_or_else(|| CrivError::new(format!("note `{id}` does not resolve")))?;
    let mut rows = Vec::new();
    for diagram in &note.c4_diagrams {
        for relationship in &diagram.relationships {
            rows.push(format!(
                "level={} from={} to={} label={}",
                diagram.level.as_str(),
                relationship.from,
                relationship.to,
                relationship.label.as_deref().unwrap_or("missing")
            ));
        }
    }
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
    let hash = if id == "latest" {
        fs::read_to_string(root.join(".criv/latest"))?
            .trim()
            .to_string()
    } else {
        id.to_string()
    };
    let path = is_snapshot_hash(&hash)
        .then(|| root.join(".criv/snapshots").join(format!("{hash}.json")))
        .filter(|path| path.exists());
    let contents = if let Some(path) = path {
        fs::read_to_string(&path)
            .map_err(|err| CrivError::new(format!("failed to read snapshot `{hash}`: {err}")))?
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
    let spec = format!("{id}:.criv/state.json");
    let output = Command::new("git")
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_PREFIX")
        .args(["show", &spec])
        .output()
        .map_err(|err| CrivError::new(format!("failed to invoke git for `{id}`: {err}")))?;
    if output.status.success() {
        String::from_utf8(output.stdout).map_err(|err| {
            CrivError::new(format!(
                "git ref `{id}` produced non-UTF-8 .criv/state.json: {err}"
            ))
        })
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(CrivError::new(format!(
            "snapshot or git ref `{id}` does not resolve: {}",
            stderr.trim()
        )))
    }
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
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn c4_elements_lists_resolution_status() {
        let temp = TempDir::new().unwrap();
        write_query_fixture(temp.path());
        let vault = Vault::load(temp.path()).unwrap();

        let rows = c4_elements(&vault, "c4").unwrap();

        assert_eq!(
            rows,
            vec![
                "level=container alias=cli category=container kind=Container source=src/lib.rs"
                    .to_string(),
                "level=container alias=plugin category=container kind=Container source=unresolved"
                    .to_string(),
                "level=container alias=external category=software-system kind=System_Ext source=none"
                    .to_string(),
            ]
        );
    }

    #[test]
    fn c4_relationships_lists_labels() {
        let temp = TempDir::new().unwrap();
        write_query_fixture(temp.path());
        let vault = Vault::load(temp.path()).unwrap();

        let rows = c4_relationships(&vault, "c4").unwrap();

        assert_eq!(
            rows,
            vec![
                "level=container from=cli to=plugin label=writes state for".to_string(),
                "level=container from=plugin to=external label=missing".to_string(),
            ]
        );
    }

    #[test]
    fn snapshot_hash_shape() {
        assert!(is_snapshot_hash("abc123"));
        assert!(!is_snapshot_hash("../../etc/passwd"));
        assert!(!is_snapshot_hash("HEAD~1"));
        assert!(!is_snapshot_hash(""));
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
        fs::write(
            root.join("docs/c4.md"),
            r#"---
id: c4
kind: doc
title: C4
---

# C4

```mermaid
C4Container
Container(cli, "criv CLI", "Rust", "Validates and queries the vault")
%% criv:source src/lib.rs#fn:helper
Container(plugin, "Obsidian Plugin", "TypeScript", "Reads generated state")
%% criv:source src/missing.rs
System_Ext(external, "GitHub", "Renders Mermaid")
Rel(cli, plugin, "writes state for")
Rel(plugin, external)
```
"#,
        )
        .unwrap();
    }
}
