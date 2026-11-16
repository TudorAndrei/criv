//! `criv adr new`: the draft file of a new dated decision.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::identity::{self, ADR_SECTIONS};
use crate::repository::RepositoryFiles;
use crate::vault::{LoadOptions, SourceLoad, Vault};
use crate::{CrivError, Result};

/// Writes the draft of a new decision titled `title` on `date` (`YYYYMMDD`,
/// today's UTC date when absent) and returns its path below the repository
/// root. Nothing is written when the vault already holds the id.
pub fn create(files: &RepositoryFiles, title: &str, date: Option<&str>) -> Result<String> {
    let date = date.map_or_else(today, ToOwned::to_owned);
    let id = identity::dated_adr_id(&date, title).map_err(CrivError::new)?;
    let vault = Vault::load(
        files,
        LoadOptions {
            source: SourceLoad::Skip,
            ..LoadOptions::default()
        },
    )?;
    let prefix = vault.config().adr_prefix();
    let path = format!("{prefix}{}.md", id.strip_prefix("ADR-").unwrap_or(&id));
    let holder = vault
        .notes()
        .iter()
        .find(|note| note.id() == Some(id.as_str()))
        .map(|note| note.rel_path().to_owned());
    let holder = match holder {
        Some(holder) => Some(holder),
        None => files.file_exists(Path::new(&path))?.then(|| path.clone()),
    };
    if let Some(holder) = holder {
        return Err(taken(&id, &holder));
    }

    let contents = draft(&id, title, &date)?;
    let adr_dir = prefix.strip_suffix('/').unwrap_or(".");
    if !files
        .write_scope(Path::new(adr_dir))?
        .write_new(Path::new(&path), &contents)?
    {
        return Err(taken(&id, &path));
    }
    Ok(path)
}

fn taken(id: &str, holder: &str) -> CrivError {
    CrivError::new(format!(
        "ADR id `{id}` already exists in the vault at `{holder}`; give the new decision a more specific title"
    ))
}

/// The frontmatter of a new draft decision, in field order.
#[derive(Debug, Serialize)]
struct DraftFrontmatter<'a> {
    id: &'a str,
    kind: &'static str,
    title: &'a str,
    status: &'static str,
    date: String,
}

/// The draft note: frontmatter, the title heading, and the empty ADR sections.
fn draft(id: &str, title: &str, date: &str) -> Result<String> {
    let frontmatter = serde_norway::to_string(&DraftFrontmatter {
        id,
        kind: "decision",
        title,
        status: "draft",
        date: iso_date(date),
    })
    .map_err(|err| CrivError::new(format!("failed to serialize the ADR frontmatter: {err}")))?;
    let mut contents = format!("---\n{frontmatter}---\n\n# {title}\n");
    for section in ADR_SECTIONS {
        contents.push_str("\n## ");
        contents.push_str(section);
        contents.push('\n');
    }
    Ok(contents)
}

/// `YYYYMMDD` as `YYYY-MM-DD`.
fn iso_date(date: &str) -> String {
    date.split_at_checked(4)
        .and_then(|(year, rest)| {
            let (month, day) = rest.split_at_checked(2)?;
            Some(format!("{year}-{month}-{day}"))
        })
        .unwrap_or_else(|| date.to_owned())
}

/// Today's UTC date as `YYYYMMDD`.
fn today() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    identity::civil_date(seconds / 86_400)
}
