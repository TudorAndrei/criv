use std::path::Path;

use crate::vault::{NoteKind, ResolvedLink, Vault, source_fragment_path};
use crate::{Args, CrivError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Text,
    Json,
}

#[derive(Debug)]
pub(crate) struct QueryOptions {
    name: String,
    values: Vec<String>,
    format: Format,
}

impl QueryOptions {
    pub(crate) fn parse(mut args: Args) -> Result<Self> {
        let Some(name) = args.next() else {
            return Err(CrivError::usage("missing query name"));
        };

        let mut values = Vec::new();
        let mut format = Format::Text;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--format" => {
                    format = match args.expect_value("--format")?.as_str() {
                        "text" => Format::Text,
                        "json" => Format::Json,
                        value => {
                            return Err(CrivError::usage(format!(
                                "unsupported query format `{value}`"
                            )));
                        }
                    };
                }
                other => values.push(other.to_string()),
            }
        }

        Ok(Self {
            name,
            values,
            format,
        })
    }
}

pub(crate) fn run(root: &Path, options: QueryOptions) -> Result<()> {
    let vault = Vault::load(root)?;
    let rows = match options.name.as_str() {
        "next-adr-id" => vec![next_adr_id(&vault)],
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
        "coverage" => coverage(&vault),
        "nodes" => nodes(&vault, &options.values),
        other => {
            return Err(CrivError::usage(format!(
                "query `{other}` is not implemented in this MVP"
            )));
        }
    };

    print_rows(&rows, options.format);
    Ok(())
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
            if let ResolvedLink::Note { id } = vault.resolve_link(&link.target) {
                if id == target_id {
                    rows.push(note.display_id().to_string());
                    break;
                }
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
    let Some((path, _)) = vault.resolve_source_path(source_fragment_path(symbol)) else {
        return Vec::new();
    };

    let mut rows = Vec::new();
    for note in &vault.notes {
        let frontmatter_refs = note
            .targets_symbols
            .iter()
            .filter_map(|target| vault.resolve_source_path(source_fragment_path(target)))
            .any(|(target_path, _)| target_path == path);
        let body_refs = note.wiki_links.iter().any(|link| {
            matches!(
                vault.resolve_link(&link.target),
                ResolvedLink::Source {
                    path: resolved_path,
                    ..
                } if resolved_path == path
            )
        });
        if frontmatter_refs || body_refs {
            rows.push(note.display_id().to_string());
        }
    }
    rows.sort();
    rows.dedup();
    rows
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

fn coverage(vault: &Vault) -> Vec<String> {
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
    vec![
        format!("source_files={}", vault.source_files().len()),
        format!("governed_files={}", governed.len()),
        format!(
            "ungoverned_files={}",
            vault.source_files().len().saturating_sub(governed.len())
        ),
    ]
}

fn nodes(vault: &Vault, values: &[String]) -> Vec<String> {
    let mut kind = None;
    let mut without_docs = false;
    let mut index = 0;
    while index < values.len() {
        match values[index].as_str() {
            "--kind" => {
                kind = values.get(index + 1).map(String::as_str);
                index += 2;
            }
            "--without-docs" => {
                without_docs = true;
                index += 1;
            }
            _ => index += 1,
        }
    }

    let mut rows = Vec::new();
    match kind {
        Some("code") => {
            for source_file in vault.source_files() {
                if without_docs && !references(vault, source_file).is_empty() {
                    continue;
                }
                rows.push(source_file.clone());
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

fn print_rows(rows: &[String], format: Format) {
    match format {
        Format::Text => {
            for row in rows {
                println!("{row}");
            }
        }
        Format::Json => {
            println!("[");
            for (index, row) in rows.iter().enumerate() {
                let comma = if index + 1 == rows.len() { "" } else { "," };
                println!("  \"{}\"{}", json_escape(row), comma);
            }
            println!("]");
        }
    }
}

fn json_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
