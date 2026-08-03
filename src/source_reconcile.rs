use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::Range;
use std::path::Path;

use clap::Args as ClapArgs;
use serde::{Deserialize, Serialize};

use crate::git::{self, ChangeStatus, ChangedEntry, ChangedSet};
use crate::util::{
    file_permissions_in, remove_file_in, write_atomic_in, write_atomic_with_permissions_in,
};
use crate::vault::Vault;
use crate::{CrivError, Result};

const RECEIPT_SCHEMA: &str = "criv.source-reconcile/1";
const RECEIPT_PATH: &str = ".criv/source-reconcile.json";
const COMMIT_MESSAGE: &str = "docs(adr): reconcile renamed source scopes";

#[derive(Debug, ClapArgs)]
pub(crate) struct Options {
    #[arg(long, help = "Target branch or commit to compare with")]
    base: String,
    #[arg(
        long,
        help = "Report a required reconciliation without modifying the worktree"
    )]
    check: bool,
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
struct Mapping {
    old_path: String,
    new_path: String,
}

#[derive(Debug, Clone)]
struct PlannedFile {
    path: String,
    before: String,
    after: String,
}

#[derive(Debug)]
struct Plan {
    base_ref: String,
    target_sha: String,
    head_sha: String,
    merge_base: String,
    mappings: Vec<Mapping>,
    files: Vec<PlannedFile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Receipt {
    schema: String,
    base_ref: String,
    target_sha: String,
    head_sha: String,
    merge_base: String,
    mappings: Vec<Mapping>,
    files: Vec<ReceiptFile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ReceiptFile {
    path: String,
    before_hash: String,
    after_hash: String,
}

struct TransactionSnapshot {
    index_tree: String,
    files: Vec<PathSnapshot>,
    receipt: PathSnapshot,
}

struct PathSnapshot {
    path: String,
    contents: Option<String>,
    permissions: Option<fs::Permissions>,
}

#[derive(Debug, Clone, Copy)]
enum ScalarStyle {
    Plain,
    SingleQuoted,
    DoubleQuoted,
}

#[derive(Debug)]
struct ScalarSpan {
    value: String,
    range: Range<usize>,
    style: ScalarStyle,
}

pub(crate) fn run(root: &Path, options: Options) -> Result<()> {
    if !git::is_repository(root)? {
        return Err(CrivError::new(
            "`criv adr reconcile-sources` requires a Git worktree",
        ));
    }
    let target_sha = git::resolve_commit(root, &options.base)?;
    let plan = build_plan(root, &options.base, &target_sha)?;
    println!("source reconciliation target: {}", plan.target_sha);
    if plan.files.is_empty() {
        println!("governed source paths are current; no reconciliation is required");
        return Ok(());
    }
    print_plan(&plan);
    if options.check {
        return Err(CrivError::new(format!(
            "governed source paths require reconciliation; run `criv adr reconcile-sources --base {}`",
            options.base
        )));
    }
    let dirty = git::dirty_paths(root)?;
    if !dirty.is_empty() {
        return Err(CrivError::new(format!(
            "refusing to reconcile a dirty worktree; commit or stash: {}",
            dirty.join(", ")
        )));
    }
    git::preflight_commit_identity(root)?;
    ensure_stable_base(root, &plan, "before source reconciliation")?;
    let paths = plan
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let snapshot = TransactionSnapshot::capture(root, &paths)?;
    let result = (|| {
        apply_plan(root, &plan)?;
        ensure_stable_base(root, &plan, "during source reconciliation")?;
        git::stage_paths(root, &paths)?;
        let staged = git::staged_changes(root)?;
        if !receipt_allows_transaction(root, &staged.entries) {
            return Err(CrivError::new(
                "source reconciliation receipt does not prove the complete staged transaction",
            ));
        }
        ensure_stable_base(root, &plan, "before the source reconciliation commit")?;
        git::commit_staged(root, COMMIT_MESSAGE)
    })();
    let commit = match result {
        Ok(commit) => commit,
        Err(error) => return Err(snapshot.rollback(root, error)),
    };
    println!("source reconciliation committed: {commit}");
    Ok(())
}

fn build_plan(root: &Path, base_ref: &str, target_sha: &str) -> Result<Plan> {
    let vault = Vault::load(root)?;
    let head_sha = git::resolve_commit(root, "HEAD")?;
    let merge_base = git::merge_base(root, target_sha, &head_sha)?;
    let changes = git::changes_between(root, &merge_base, &head_sha)?;
    let mappings = rename_mappings(&changes)?;
    let mapping_by_old = mappings
        .iter()
        .map(|mapping| (mapping.old_path.as_str(), mapping.new_path.as_str()))
        .collect::<BTreeMap<_, _>>();
    let deleted = changes
        .entries
        .iter()
        .filter(|entry| entry.status == ChangeStatus::Deleted)
        .map(|entry| entry.path.as_str())
        .collect::<BTreeSet<_>>();
    let copied = changes
        .entries
        .iter()
        .filter(|entry| entry.status == ChangeStatus::Copied)
        .filter_map(|entry| entry.previous_path.as_deref())
        .collect::<BTreeSet<_>>();
    let current_sources = vault
        .source_files()
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut required = BTreeMap::new();

    for note in vault
        .notes
        .iter()
        .filter(|note| vault.is_effective_decision(note))
    {
        let matches = vault.source_globs_have_matches(&note.governs);
        for (governs, has_match) in note.governs.iter().zip(matches) {
            if has_match {
                continue;
            }
            if let Some(new_path) = mapping_by_old.get(governs.as_str()) {
                if !current_sources.contains(new_path) {
                    return Err(CrivError::new(format!(
                        "cannot reconcile `{governs}` to `{new_path}` because the destination is not in the current source catalog"
                    )));
                }
                required.insert(governs.clone(), (*new_path).to_string());
                continue;
            }
            if deleted.contains(governs.as_str()) {
                return Err(successor_required(governs, "was deleted"));
            }
            if copied.contains(governs.as_str()) {
                return Err(successor_required(
                    governs,
                    "was copied rather than renamed",
                ));
            }
            return Err(successor_required(
                governs,
                "is unresolved and has no one-to-one Git rename",
            ));
        }
    }

    let mut files = Vec::new();
    if !required.is_empty() {
        for note in vault
            .notes
            .iter()
            .filter(|note| vault.is_effective_decision(note))
        {
            if !note.governs.iter().any(|path| required.contains_key(path)) {
                continue;
            }
            let before = fs::read_to_string(&note.path)?;
            let after = rewrite_governs(&before, &required)?;
            if before != after {
                files.push(PlannedFile {
                    path: note.rel_path.clone(),
                    before,
                    after,
                });
            }
        }
    }
    let mut used = BTreeSet::new();
    for file in &files {
        used.extend(changed_governs_values(
            &file.before,
            &file.after,
            &required,
        )?);
    }
    let expected = required.keys().cloned().collect::<BTreeSet<_>>();
    if used != expected {
        return Err(CrivError::new(format!(
            "unsupported `governs:` YAML prevented exact source reconciliation: {}",
            expected
                .difference(&used)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let mappings = required
        .into_iter()
        .map(|(old_path, new_path)| Mapping { old_path, new_path })
        .collect();
    Ok(Plan {
        base_ref: base_ref.to_string(),
        target_sha: target_sha.to_string(),
        head_sha,
        merge_base,
        mappings,
        files,
    })
}

fn successor_required(path: &str, reason: &str) -> CrivError {
    CrivError::new(format!(
        "cannot reconcile governed source `{path}` because it {reason}; add a new accepted ADR with `supersedes:` and the surviving scope"
    ))
}

fn rename_mappings(changes: &ChangedSet) -> Result<Vec<Mapping>> {
    let mut by_old = BTreeMap::new();
    let mut by_new = BTreeMap::new();
    for entry in &changes.entries {
        if entry.status != ChangeStatus::Renamed {
            continue;
        }
        let old_path = entry.previous_path.as_ref().ok_or_else(|| {
            CrivError::new(format!(
                "Git rename for `{}` has no source path",
                entry.path
            ))
        })?;
        if by_old
            .insert(old_path.clone(), entry.path.clone())
            .is_some()
            || by_new
                .insert(entry.path.clone(), old_path.clone())
                .is_some()
        {
            return Err(CrivError::new(format!(
                "ambiguous Git rename mapping involving `{old_path}` and `{}`",
                entry.path
            )));
        }
    }
    Ok(by_old
        .into_iter()
        .map(|(old_path, new_path)| Mapping { old_path, new_path })
        .collect())
}

fn print_plan(plan: &Plan) {
    println!("source reconciliation mapping:");
    for mapping in &plan.mappings {
        println!("  {} -> {}", mapping.old_path, mapping.new_path);
    }
    println!(
        "ADR files: {}",
        plan.files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

fn ensure_stable_base(root: &Path, plan: &Plan, moment: &str) -> Result<()> {
    if git::ref_is_stable(root, &plan.base_ref, &plan.target_sha)? {
        Ok(())
    } else {
        Err(CrivError::new(format!(
            "target ref `{}` moved {moment}; retry against its new SHA",
            plan.base_ref
        )))
    }
}

fn apply_plan(root: &Path, plan: &Plan) -> Result<()> {
    for file in &plan.files {
        write_atomic_in(root, Path::new("."), Path::new(&file.path), &file.after)?;
    }
    let errors = crate::check::validate_all(root)?
        .into_iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(|diagnostic| diagnostic.describe())
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        return Err(CrivError::new(format!(
            "source reconciliation wrote files but vault validation failed:\n{}",
            errors.join("\n")
        )));
    }
    let receipt = Receipt {
        schema: RECEIPT_SCHEMA.into(),
        base_ref: plan.base_ref.clone(),
        target_sha: plan.target_sha.clone(),
        head_sha: plan.head_sha.clone(),
        merge_base: plan.merge_base.clone(),
        mappings: plan.mappings.clone(),
        files: plan
            .files
            .iter()
            .map(|file| ReceiptFile {
                path: file.path.clone(),
                before_hash: hash(&file.before),
                after_hash: hash(&file.after),
            })
            .collect(),
    };
    let contents = serde_json::to_string_pretty(&receipt).map_err(|error| {
        CrivError::new(format!(
            "cannot serialize source reconciliation receipt: {error}"
        ))
    })? + "\n";
    write_atomic_in(root, Path::new(".criv"), Path::new(RECEIPT_PATH), &contents)
}

pub(crate) fn receipt_is_current(root: &Path) -> bool {
    read_receipt(root).is_ok_and(|receipt| {
        receipt.schema == RECEIPT_SCHEMA
            && git::resolve_commit(root, "HEAD").ok().as_deref() == Some(receipt.head_sha.as_str())
    })
}

pub(crate) fn receipt_allows_transaction(root: &Path, entries: &[ChangedEntry]) -> bool {
    let Ok(receipt) = read_receipt(root) else {
        return false;
    };
    if receipt.schema != RECEIPT_SCHEMA
        || git::resolve_commit(root, "HEAD").ok().as_deref() != Some(receipt.head_sha.as_str())
        || !git::ref_is_stable(root, &receipt.base_ref, &receipt.target_sha).unwrap_or(false)
        || !mappings_proven(
            root,
            &receipt.merge_base,
            &receipt.head_sha,
            &receipt.mappings,
        )
    {
        return false;
    }
    let expected = receipt
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();
    let actual = entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect::<BTreeSet<_>>();
    expected == actual
        && entries
            .iter()
            .all(|entry| entry.status == ChangeStatus::Modified && entry.previous_path.is_none())
        && receipt.files.iter().all(|file| {
            git::blob(root, &receipt.head_sha, &file.path)
                .is_ok_and(|contents| hash(&contents) == file.before_hash)
                && git::blob(root, ":", &file.path)
                    .is_ok_and(|contents| hash(&contents) == file.after_hash)
                && transition_matches(
                    &git::blob(root, &receipt.head_sha, &file.path).unwrap_or_default(),
                    &git::blob(root, ":", &file.path).unwrap_or_default(),
                    &receipt.mappings,
                )
        })
}

pub(crate) fn allows_history_change(
    root: &Path,
    changes: &ChangedSet,
    entry: &ChangedEntry,
) -> bool {
    if entry.status != ChangeStatus::Modified || entry.previous_path.is_some() {
        return false;
    }
    let Ok(mappings) = composed_history_mappings(&changes.entries) else {
        return false;
    };
    if mappings.is_empty() {
        return false;
    }
    let Some(old_ref) = entry.old_ref.as_deref().or(changes.old_ref.as_deref()) else {
        return false;
    };
    let new_ref = entry.new_ref.as_deref().or(changes.new_ref.as_deref());
    let Ok(old) = git::blob(root, old_ref, &entry.path) else {
        return false;
    };
    let new = match new_ref {
        Some(reference) => git::blob(root, reference, &entry.path),
        None => fs::read_to_string(root.join(&entry.path)).map_err(Into::into),
    };
    new.is_ok_and(|new| transition_matches(&old, &new, &mappings))
}

fn composed_history_mappings(entries: &[ChangedEntry]) -> Result<Vec<Mapping>> {
    let direct = rename_mappings(&ChangedSet {
        entries: entries.to_vec(),
        old_ref: None,
        new_ref: None,
        basis: String::new(),
    })?;
    let by_old = direct
        .iter()
        .map(|mapping| (mapping.old_path.as_str(), mapping.new_path.as_str()))
        .collect::<BTreeMap<_, _>>();
    let destinations = direct
        .iter()
        .map(|mapping| mapping.new_path.as_str())
        .collect::<BTreeSet<_>>();
    let mut result = Vec::new();
    for mapping in direct
        .iter()
        .filter(|mapping| !destinations.contains(mapping.old_path.as_str()))
    {
        let mut destination = mapping.new_path.as_str();
        let mut seen = BTreeSet::from([mapping.old_path.as_str()]);
        while let Some(next) = by_old.get(destination) {
            if !seen.insert(destination) {
                return Err(CrivError::new("cyclic Git rename mapping"));
            }
            destination = next;
        }
        result.push(Mapping {
            old_path: mapping.old_path.clone(),
            new_path: destination.to_string(),
        });
    }
    Ok(result)
}

fn mappings_proven(root: &Path, old_ref: &str, new_ref: &str, expected: &[Mapping]) -> bool {
    git::changes_between(root, old_ref, new_ref)
        .and_then(|changes| rename_mappings(&changes))
        .is_ok_and(|actual| {
            let actual = actual.into_iter().collect::<BTreeSet<_>>();
            expected
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
                .is_subset(&actual)
        })
}

fn transition_matches(old: &str, new: &str, mappings: &[Mapping]) -> bool {
    if top_level_scalar(old, "status").as_deref() != Some("accepted")
        || top_level_scalar(new, "status").as_deref() != Some("accepted")
    {
        return false;
    }
    let replacements = mappings
        .iter()
        .map(|mapping| (mapping.old_path.clone(), mapping.new_path.clone()))
        .collect::<BTreeMap<_, _>>();
    rewrite_governs(old, &replacements).is_ok_and(|rewritten| rewritten == new && rewritten != old)
}

fn rewrite_governs(contents: &str, replacements: &BTreeMap<String, String>) -> Result<String> {
    let spans = governs_scalars(contents)?;
    let mut output = contents.to_string();
    for span in spans.iter().rev() {
        if let Some(replacement) = replacements.get(&span.value) {
            output.replace_range(span.range.clone(), &render_scalar(replacement, span.style));
        }
    }
    Ok(output)
}

fn changed_governs_values(
    old: &str,
    new: &str,
    replacements: &BTreeMap<String, String>,
) -> Result<BTreeSet<String>> {
    let spans = governs_scalars(old)?;
    let mut used = BTreeSet::new();
    for span in spans {
        if replacements.contains_key(&span.value) {
            used.insert(span.value);
        }
    }
    if rewrite_governs(old, replacements)? != new {
        return Err(CrivError::new(
            "source reconciliation produced a non-exact `governs:` rewrite",
        ));
    }
    Ok(used)
}

fn governs_scalars(contents: &str) -> Result<Vec<ScalarSpan>> {
    let lines = indexed_lines(contents);
    let Some((frontmatter_start, frontmatter_end)) = frontmatter_bounds(&lines) else {
        return Err(CrivError::new("ADR has no closed YAML frontmatter"));
    };
    let mut field = None;
    for (index, (_, line)) in lines
        .iter()
        .enumerate()
        .take(frontmatter_end)
        .skip(frontmatter_start)
    {
        let line = *line;
        if line.starts_with(char::is_whitespace)
            || line.trim().is_empty()
            || line.trim_start().starts_with('#')
        {
            continue;
        }
        if line == "governs:" {
            if field.replace(index).is_some() {
                return Err(CrivError::new("ADR has duplicate `governs:` fields"));
            }
        } else if line.starts_with("governs:") {
            return Err(CrivError::new("`governs:` must use a block YAML sequence"));
        }
    }
    let Some(field) = field else {
        return Ok(Vec::new());
    };
    let mut spans = Vec::new();
    for (line_start, line) in lines.iter().take(frontmatter_end).skip(field + 1) {
        if !line.starts_with(char::is_whitespace) && !line.trim().is_empty() {
            break;
        }
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some(raw) = trimmed.strip_prefix("- ") else {
            return Err(CrivError::new("`governs:` contains unsupported YAML"));
        };
        let raw = raw.trim_end();
        if raw.is_empty() {
            return Err(CrivError::new("`governs:` contains an empty scalar"));
        }
        let value = parse_scalar(raw)?;
        let style = if raw.starts_with('\'') {
            ScalarStyle::SingleQuoted
        } else if raw.starts_with('"') {
            ScalarStyle::DoubleQuoted
        } else {
            ScalarStyle::Plain
        };
        let raw_offset = line
            .find(raw)
            .ok_or_else(|| CrivError::new("cannot locate `governs:` scalar"))?;
        spans.push(ScalarSpan {
            value,
            range: line_start + raw_offset..line_start + raw_offset + raw.len(),
            style,
        });
    }
    Ok(spans)
}

fn indexed_lines(contents: &str) -> Vec<(usize, &str)> {
    let mut offset = 0;
    contents
        .split_inclusive('\n')
        .map(|line| {
            let start = offset;
            offset += line.len();
            (start, line.trim_end_matches(['\r', '\n']))
        })
        .collect()
}

fn frontmatter_bounds(lines: &[(usize, &str)]) -> Option<(usize, usize)> {
    if lines.first().map(|line| line.1) != Some("---") {
        return None;
    }
    lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, line)| line.1 == "---")
        .map(|(end, _)| (1, end))
}

fn parse_scalar(raw: &str) -> Result<String> {
    if !raw.starts_with(['\'', '"'])
        && (raw.contains(" #") || raw.starts_with(['[', '{', '&', '*', '!', '|', '>', '@', '`']))
    {
        return Err(CrivError::new(
            "`governs:` contains an unsupported YAML scalar",
        ));
    }
    serde_norway::from_str::<String>(raw)
        .map_err(|error| CrivError::new(format!("cannot parse `governs:` scalar: {error}")))
}

fn render_scalar(value: &str, style: ScalarStyle) -> String {
    match style {
        ScalarStyle::Plain => value.to_string(),
        ScalarStyle::SingleQuoted => format!("'{}'", value.replace('\'', "''")),
        ScalarStyle::DoubleQuoted => {
            format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
        }
    }
}

fn top_level_scalar(contents: &str, key: &str) -> Option<String> {
    let lines = indexed_lines(contents);
    let (start, end) = frontmatter_bounds(&lines)?;
    let prefix = format!("{key}:");
    lines[start..end]
        .iter()
        .filter(|(_, line)| !line.starts_with(char::is_whitespace))
        .find_map(|(_, line)| line.strip_prefix(&prefix).map(str::trim))
        .and_then(|raw| serde_norway::from_str::<String>(raw).ok())
}

fn read_receipt(root: &Path) -> Result<Receipt> {
    let contents = fs::read_to_string(root.join(RECEIPT_PATH))
        .map_err(|_| CrivError::new("source reconciliation receipt is unavailable"))?;
    serde_json::from_str(&contents)
        .map_err(|_| CrivError::new("source reconciliation receipt is malformed"))
}

fn hash(contents: &str) -> String {
    blake3::hash(contents.as_bytes()).to_hex().to_string()
}

impl TransactionSnapshot {
    fn capture(root: &Path, paths: &[String]) -> Result<Self> {
        Ok(Self {
            index_tree: git::index_tree(root)?,
            files: paths
                .iter()
                .map(|path| PathSnapshot::capture(root, path))
                .collect::<Result<Vec<_>>>()?,
            receipt: PathSnapshot::capture(root, RECEIPT_PATH)?,
        })
    }

    fn rollback(self, root: &Path, error: CrivError) -> CrivError {
        let mut errors = Vec::new();
        if let Err(rollback) = git::restore_index_tree(root, &self.index_tree) {
            errors.push(rollback.to_string());
        }
        for file in self.files {
            if let Err(rollback) = file.restore(root) {
                errors.push(rollback.to_string());
            }
        }
        if let Err(rollback) = self.receipt.restore(root) {
            errors.push(rollback.to_string());
        }
        if errors.is_empty() {
            error
        } else {
            CrivError::new(format!(
                "{error}\nsource reconciliation rollback also failed:\n{}",
                errors.join("\n")
            ))
        }
    }
}

impl PathSnapshot {
    fn capture(root: &Path, path: &str) -> Result<Self> {
        if root.join(path).exists() {
            Ok(Self {
                path: path.to_string(),
                contents: Some(fs::read_to_string(root.join(path))?),
                permissions: Some(file_permissions_in(root, Path::new(path))?),
            })
        } else {
            Ok(Self {
                path: path.to_string(),
                contents: None,
                permissions: None,
            })
        }
    }

    fn restore(self, root: &Path) -> Result<()> {
        match (self.contents, self.permissions) {
            (Some(contents), Some(permissions)) => write_atomic_with_permissions_in(
                root,
                Path::new("."),
                Path::new(&self.path),
                &contents,
                permissions,
            ),
            (None, None) if root.join(&self.path).exists() => {
                remove_file_in(root, Path::new("."), Path::new(&self.path))
            }
            (None, None) => Ok(()),
            _ => Err(CrivError::new(format!(
                "cannot restore incomplete snapshot for `{}`",
                self.path
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn rewrites_plain_and_quoted_governs_scalars_only() {
        let input = "---\nid: ADR-0001\nstatus: accepted\ngoverns:\n  - src/old.rs\n  - 'src/second.rs'\n  - \"src/third.rs\"\n---\n\nsrc/old.rs stays in prose\n";
        let replacements = BTreeMap::from([
            ("src/old.rs".into(), "src/new.rs".into()),
            ("src/second.rs".into(), "src/second-new.rs".into()),
            ("src/third.rs".into(), "src/third-new.rs".into()),
        ]);
        let output = rewrite_governs(input, &replacements).unwrap();
        assert!(output.contains("  - src/new.rs\n"));
        assert!(output.contains("  - 'src/second-new.rs'\n"));
        assert!(output.contains("  - \"src/third-new.rs\"\n"));
        assert!(output.ends_with("src/old.rs stays in prose\n"));
    }

    #[test]
    fn refuses_inline_or_nested_governs_yaml() {
        let inline = "---\nstatus: accepted\ngoverns: [src/old.rs]\n---\n";
        assert!(rewrite_governs(inline, &BTreeMap::new()).is_err());
        let nested = "---\nstatus: accepted\ngoverns:\n  path: src/old.rs\n---\n";
        assert!(rewrite_governs(nested, &BTreeMap::new()).is_err());
    }

    #[test]
    fn history_transition_rejects_any_non_governs_change() {
        let old =
            "---\nid: ADR-0001\nstatus: accepted\ngoverns:\n  - src/old.rs\n---\n\nDecision\n";
        let new = old.replace("src/old.rs", "src/new.rs");
        let mapping = [Mapping {
            old_path: "src/old.rs".into(),
            new_path: "src/new.rs".into(),
        }];
        assert!(transition_matches(old, &new, &mapping));
        assert!(!transition_matches(old, &(new + "changed\n"), &mapping));
    }

    #[test]
    fn composes_chained_history_renames() {
        let entries = [
            ChangedEntry {
                status: ChangeStatus::Renamed,
                path: "src/middle.rs".into(),
                previous_path: Some("src/old.rs".into()),
                old_ref: None,
                new_ref: None,
            },
            ChangedEntry {
                status: ChangeStatus::Renamed,
                path: "src/new.rs".into(),
                previous_path: Some("src/middle.rs".into()),
                old_ref: None,
                new_ref: None,
            },
        ];
        assert_eq!(
            composed_history_mappings(&entries).unwrap(),
            vec![Mapping {
                old_path: "src/old.rs".into(),
                new_path: "src/new.rs".into(),
            }]
        );
    }

    #[test]
    fn excludes_copies_and_rejects_ambiguous_renames() {
        let copy = ChangedEntry {
            status: ChangeStatus::Copied,
            path: "src/copy.rs".into(),
            previous_path: Some("src/old.rs".into()),
            old_ref: None,
            new_ref: None,
        };
        let copied = ChangedSet {
            entries: vec![copy],
            old_ref: None,
            new_ref: None,
            basis: String::new(),
        };
        assert!(rename_mappings(&copied).unwrap().is_empty());

        let ambiguous = ChangedSet {
            entries: vec![
                ChangedEntry {
                    status: ChangeStatus::Renamed,
                    path: "src/one.rs".into(),
                    previous_path: Some("src/old.rs".into()),
                    old_ref: None,
                    new_ref: None,
                },
                ChangedEntry {
                    status: ChangeStatus::Renamed,
                    path: "src/two.rs".into(),
                    previous_path: Some("src/old.rs".into()),
                    old_ref: None,
                    new_ref: None,
                },
            ],
            old_ref: None,
            new_ref: None,
            basis: String::new(),
        };
        assert!(rename_mappings(&ambiguous).is_err());
    }

    #[test]
    fn preserves_case_sensitive_path_replacements() {
        let old = "---\nstatus: accepted\ngoverns:\n  - src/name.rs\n---\n";
        let new = "---\nstatus: accepted\ngoverns:\n  - src/Name.rs\n---\n";
        let mappings = [Mapping {
            old_path: "src/name.rs".into(),
            new_path: "src/Name.rs".into(),
        }];
        assert!(transition_matches(old, new, &mappings));
    }

    #[test]
    fn rejects_a_base_ref_that_moved_after_planning() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();
        run_git(root, &["init", "-b", "main"]);
        run_git(root, &["config", "user.email", "criv@example.com"]);
        run_git(root, &["config", "user.name", "criv"]);
        fs::write(root.join("one"), "one\n").unwrap();
        run_git(root, &["add", "one"]);
        run_git(root, &["commit", "-m", "one"]);
        let target_sha = git::resolve_commit(root, "main").unwrap();
        let plan = Plan {
            base_ref: "main".into(),
            target_sha,
            head_sha: git::resolve_commit(root, "HEAD").unwrap(),
            merge_base: git::resolve_commit(root, "HEAD").unwrap(),
            mappings: Vec::new(),
            files: Vec::new(),
        };
        fs::write(root.join("two"), "two\n").unwrap();
        run_git(root, &["add", "two"]);
        run_git(root, &["commit", "-m", "two"]);

        assert!(ensure_stable_base(root, &plan, "during test").is_err());
    }

    fn run_git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
    }
}
