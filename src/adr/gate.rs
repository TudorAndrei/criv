//! The ADR immutability gate that enforcement runs on a changed set.

use std::collections::{BTreeMap, BTreeSet};

use super::receipt::{self, IdentityReceipt, Receipt, SourceReceipt};
use crate::config::{CONFIG_FILE, Config, Stage};
use crate::git::{ChangeStatus, ChangedEntry, ChangedSet, GitRepository, Revision};
use crate::repository::RepositoryFiles;
use crate::{identity, markdown};

/// Validate the complete selected ADR transaction in enforcement order.
pub fn change_violations(
    files: &RepositoryFiles,
    config: &Config,
    changes: Option<&ChangedSet>,
    stage: Stage,
) -> Vec<String> {
    let Some(git) = GitRepository::discover(files.root()).ok().flatten() else {
        return immutability_violations(config, changes, |_| false);
    };
    let identity = IdentityReceipt::read(files);
    let source = SourceReceipt::read(files);
    let gate = Gate {
        files,
        git: &git,
        changes,
        stage,
        renames: changes
            .and_then(|changes| receipt::composed_renames(&changes.entries).ok())
            .unwrap_or_default(),
        identity: identity
            .as_ref()
            .ok()
            .and_then(Option::as_ref)
            .map(|receipt| IdentityProof::new(receipt, &git, changes)),
    };
    let proofs = [gate.proof(&identity), gate.proof(&source)];
    let staged = proofs.iter().any(|proof| proof.staged);
    let mut violations = immutability_violations(config, changes, |entry| {
        staged
            || gate.allows(entry)
            || (stage == Stage::Ci && gate.is_branch_local_ci_change(entry))
    });
    violations.extend(proofs.into_iter().filter_map(|proof| proof.failure));
    violations.extend(gate.scope_violation(config));
    violations
}

struct Gate<'a> {
    files: &'a RepositoryFiles,
    git: &'a GitRepository,
    changes: Option<&'a ChangedSet>,
    stage: Stage,
    /// The Git renames composed across the compared history.
    renames: BTreeMap<String, String>,
    identity: Option<IdentityProof<'a>>,
}

/// What one receipt proves about the selected transaction.
struct Proof {
    /// The receipt is exactly the staged commit transaction.
    staged: bool,
    failure: Option<String>,
}

/// The identity receipt's proofs, each computed once for the gate.
struct IdentityProof<'a> {
    receipt: &'a IdentityReceipt,
    index: bool,
    history: bool,
    /// The compared commits that are exactly the receipt's transaction.
    commits: BTreeSet<&'a str>,
}

impl<'a> IdentityProof<'a> {
    fn new(
        receipt: &'a IdentityReceipt,
        git: &GitRepository,
        changes: Option<&'a ChangedSet>,
    ) -> Self {
        let entries = changes.map_or(&[][..], |changes| changes.entries.as_slice());
        Self {
            receipt,
            index: receipt.proves_index(git),
            history: changes.is_some_and(|changes| receipt.proves_history(git, changes)),
            commits: entries
                .iter()
                .filter_map(|entry| match &entry.new {
                    Revision::Commit(commit) => Some(commit.as_str()),
                    _ => None,
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .filter(|commit| receipt.allows_commit(git, commit))
                .collect(),
        }
    }
}

impl Gate<'_> {
    /// A malformed receipt, or a current one that does not prove the staged
    /// transaction, is a violation.
    fn proof<R: Receipt>(&self, receipt: &crate::Result<Option<R>>) -> Proof {
        let receipt = match receipt {
            Ok(Some(receipt)) => receipt,
            Ok(None) => {
                return Proof {
                    staged: false,
                    failure: None,
                };
            }
            Err(error) => {
                return Proof {
                    staged: false,
                    failure: Some(error.to_string()),
                };
            }
        };
        let staged = self.stage == Stage::Commit
            && self
                .changes
                .is_some_and(|changes| receipt.allows_transaction(self.git, &changes.entries));
        Proof {
            staged,
            failure: (!staged && receipt.planned_from_head(self.git)).then(|| {
                format!(
                    "{} receipt does not prove the complete staged transaction",
                    R::NAME
                )
            }),
        }
    }

    /// Accept receipt-proven changes or an exact portable-link migration.
    fn allows(&self, entry: &ChangedEntry) -> bool {
        if receipt::source_history_allows(self.files, self.git, &self.renames, entry) {
            return true;
        }
        if let Some(proof) = &self.identity {
            // A committed receipt proves the entire tree transition, including
            // paths that Git reports as modifications when ADR mappings overlap.
            if let Revision::Commit(commit) = &entry.new
                && proof.commits.contains(commit.as_str())
            {
                return true;
            }
            if proof.history && proof.receipt.covers(entry) {
                return true;
            }
            if proof.index && proof.receipt.records_move(entry) {
                return true;
            }
        }
        entry.status == ChangeStatus::Modified && self.is_portable_link_migration(entry)
    }

    fn is_portable_link_migration(&self, entry: &ChangedEntry) -> bool {
        let Some(old_ref) = &entry.old_ref else {
            return false;
        };
        let old = self
            .git
            .text(&Revision::Commit(old_ref.clone()), &entry.path);
        let new = receipt::read_text(self.files, self.git, &entry.new, &entry.path);
        matches!((old, new), (Ok(old), Ok(new)) if is_portable_link_migration(&old, &new))
    }

    /// A CI comparison can contain a deletion for a target path that did not
    /// exist at the branch/target merge base: that is the same-path allocation
    /// race, not an edit of a published ADR. Anything present at that base
    /// stays immutable.
    fn is_branch_local_ci_change(&self, entry: &ChangedEntry) -> bool {
        let Some(target) = entry.old_ref.as_deref() else {
            return false;
        };
        let path = entry.previous_path.as_deref().unwrap_or(&entry.path);
        self.git.merge_base(target, "HEAD").is_ok_and(|merge_base| {
            matches!(self.git.file(&Revision::Commit(merge_base), path), Ok(None))
        })
    }

    /// Reject a decision edit that also moves the configured decision
    /// directory, because the gate reads the new scope.
    fn scope_violation(&self, current: &Config) -> Option<String> {
        let changes = self.changes?;
        let paths = || {
            changes
                .entries
                .iter()
                .flat_map(|entry| [Some(entry.path.as_str()), entry.previous_path.as_deref()])
                .flatten()
        };
        if !paths().any(|path| path == CONFIG_FILE) {
            return None;
        }
        // An unborn `HEAD` or a configuration added by this change has no
        // earlier scope. One that cannot be read has an unknown scope.
        let old_ref = changes.old_ref.clone()?;
        let previous = match self
            .git
            .file(&Revision::Commit(old_ref.clone()), CONFIG_FILE)
        {
            Ok(None) => return None,
            Ok(Some(file)) => String::from_utf8(file.bytes)
                .map_err(|error| error.to_string())
                .and_then(|text| Config::parse(Some(&text)).map_err(|error| error.to_string())),
            Err(error) => Err(error.to_string()),
        };
        let previous = match previous {
            Ok(previous) => previous,
            Err(error) => {
                return paths().any(|path| current.is_adr_path(path)).then(|| {
                    format!(
                        "{CONFIG_FILE} at `{old_ref}` cannot be read, so the decision scope it moves from is unknown in the same transaction as a decision change: {error}"
                    )
                });
            }
        };
        if previous.same_decision_scope(current) {
            return None;
        }
        paths()
            .any(|path| previous.is_adr_path(path) || current.is_adr_path(path))
            .then(|| {
                format!(
                    "{CONFIG_FILE} moves the decision scope from `{}/{}` to `{}/{}` in the same transaction as a decision change; the immutability gate would read the new scope",
                    previous.docs_dir(), previous.adr_dir(), current.docs_dir(), current.adr_dir()
                )
            })
    }
}

/// Report protected ADR edits in Git entry order, after testing allowed
/// exceptions.
fn immutability_violations(
    config: &Config,
    changes: Option<&ChangedSet>,
    mut is_allowed: impl FnMut(&ChangedEntry) -> bool,
) -> Vec<String> {
    let Some(changes) = changes else {
        return Vec::new();
    };
    changes
        .entries
        .iter()
        .filter(|entry| !matches!(entry.status, ChangeStatus::Added | ChangeStatus::Copied))
        .filter(|entry| config.is_adr_path(entry.previous_path.as_deref().unwrap_or(&entry.path)))
        .filter(|entry| !is_allowed(entry))
        .map(|entry| {
            let display_path = entry.previous_path.as_ref().map_or_else(
                || entry.path.clone(),
                |previous| format!("{previous} -> {}", entry.path),
            );
            format!(
                "{display_path}: ADR files are immutable; add a new ADR with `supersedes` instead of modifying an existing one"
            )
        })
        .collect()
}

fn is_portable_link_migration(old: &str, new: &str) -> bool {
    old != new && normalize_portable_adr_links(new) == old
}

/// Restore short ADR aliases while preserving all text outside matching links.
fn normalize_portable_adr_links(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut start = 0;
    for span in markdown::wiki_link_spans(text) {
        let (Some(prefix), Some(body), Some(link)) = (
            text.get(start..span.outer.start),
            text.get(span.body),
            text.get(span.outer.clone()),
        ) else {
            break;
        };
        normalized.push_str(prefix);
        if let Some(alias) = portable_adr_link_alias(body) {
            normalized.push_str("[[");
            normalized.push_str(alias);
            normalized.push_str("]]");
        } else {
            normalized.push_str(link);
        }
        start = span.outer.end;
    }
    normalized.push_str(text.get(start..).unwrap_or_default());
    normalized
}

/// Return the ADR alias only when its ID and fragment match the target.
fn portable_adr_link_alias(body: &str) -> Option<&str> {
    let (target, alias) = markdown::split_alias(body);
    let alias = alias?.trim();
    let (alias_base, alias_fragment) = markdown::split_fragment(alias);
    let number = identity::parse_adr_id(alias_base)?;
    let (target_base, target_fragment) = markdown::split_fragment(target);
    if target_fragment != alias_fragment {
        return None;
    }
    identity::parse_adr_path(target_base.trim())
        .is_some_and(|(target_number, _)| target_number == number)
        .then_some(alias)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use crate::git::Comparison;
    use crate::git::fixture::git;

    fn changes(entries: Vec<ChangedEntry>) -> ChangedSet {
        ChangedSet {
            entries,
            old_ref: Some("HEAD".into()),
            new: None,
            basis: "test".into(),
        }
    }

    fn entry(status: ChangeStatus, path: &str, previous_path: Option<&str>) -> ChangedEntry {
        ChangedEntry {
            status,
            path: path.into(),
            previous_path: previous_path.map(Into::into),
            old_ref: Some("HEAD".into()),
            new: Revision::Worktree,
        }
    }

    /// A committed ADR fixture so receipt checks compare real Git trees.
    fn decision_repository() -> tempfile::TempDir {
        let root = tempfile::TempDir::new().unwrap();
        git(root.path(), &["init", "-b", "main"]);
        fs::create_dir_all(root.path().join("docs/adr")).unwrap();
        fs::write(
            root.path().join("criv.toml"),
            "[vault]\ndocs = \"docs\"\nadr = \"adr\"\n",
        )
        .unwrap();
        fs::write(root.path().join("docs/adr/0001-base.md"), "Base decision\n").unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-m", "base"]);
        root
    }

    fn worktree_changes(root: &Path) -> ChangedSet {
        GitRepository::open(root)
            .unwrap()
            .changed_set(
                &Comparison::Worktree { old_ref: "HEAD" },
                &["docs/adr", "criv.toml"],
            )
            .unwrap()
    }

    #[test]
    fn current_receipt_failures_follow_file_order_and_precede_scope_failure() {
        let root = decision_repository();
        let files = RepositoryFiles::open(root.path()).unwrap();
        let head = git(root.path(), &["rev-parse", "HEAD"]);
        fs::create_dir_all(root.path().join(".criv")).unwrap();
        let mut receipt = serde_json::json!({
            "schema": IdentityReceipt::SCHEMA, "base_ref": "missing", "target_sha": head,
            "head_sha": head, "merge_base": head, "mappings": [], "sources": [],
            "deletions": [], "files": []
        });
        fs::write(root.path().join(IdentityReceipt::PATH), receipt.to_string()).unwrap();
        receipt["schema"] = SourceReceipt::SCHEMA.into();
        fs::write(root.path().join(SourceReceipt::PATH), receipt.to_string()).unwrap();
        fs::write(root.path().join("docs/adr/0001-base.md"), "Edited\n").unwrap();
        fs::write(root.path().join("criv.toml"), "# Changed scope\n").unwrap();
        let mut changes = worktree_changes(root.path());
        changes.entries.push(entry(
            ChangeStatus::Modified,
            "other/adr/0002-existing.md",
            None,
        ));
        let config = Config::parse(Some("[vault]\ndocs = \"other\"\n")).unwrap();
        for stage in [Stage::Commit, Stage::Push] {
            let violations = change_violations(&files, &config, Some(&changes), stage);
            assert_eq!(violations.len(), 4);
            assert!(violations[0].starts_with("other/adr/0002-existing.md:"));
            assert_eq!(
                violations[1],
                "ADR reconciliation receipt does not prove the complete staged transaction"
            );
            assert_eq!(
                violations[2],
                "source reconciliation receipt does not prove the complete staged transaction"
            );
            assert!(violations[3].starts_with("criv.toml moves the decision scope"));
        }
        assert_eq!(
            change_violations(&files, &config, None, Stage::Commit).len(),
            2
        );
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-m", "advance beyond receipts"]);
        assert!(change_violations(&files, &config, None, Stage::Commit).is_empty());
    }

    #[test]
    /// A malformed receipt proves nothing and is itself a violation.
    fn a_malformed_receipt_is_reported_and_proves_no_change() {
        let root = decision_repository();
        let files = RepositoryFiles::open(root.path()).unwrap();
        fs::create_dir_all(root.path().join(".criv")).unwrap();
        fs::write(root.path().join(IdentityReceipt::PATH), "invalid receipt").unwrap();
        let malformed =
            "ADR reconciliation receipt is malformed; remove it only after recovering the worktree";
        for stage in [Stage::Commit, Stage::Push, Stage::Ci] {
            assert_eq!(
                change_violations(&files, &Config::default(), None, stage),
                [malformed]
            );
        }
        fs::write(
            root.path().join("docs/adr/0001-base.md"),
            "Changed decision\n",
        )
        .unwrap();
        let changes = worktree_changes(root.path());
        let violations =
            change_violations(&files, &Config::default(), Some(&changes), Stage::Commit);
        assert!(violations[0].starts_with("docs/adr/0001-base.md:"));
        assert_eq!(violations[1..], [malformed]);
    }

    #[test]
    fn an_unreadable_previous_configuration_blocks_a_decision_change() {
        let root = decision_repository();
        let files = RepositoryFiles::open(root.path()).unwrap();
        fs::write(root.path().join("criv.toml"), "[patterns.old]\n").unwrap();
        git(
            root.path(),
            &["commit", "-qam", "unsupported configuration"],
        );
        fs::write(root.path().join("criv.toml"), "").unwrap();
        fs::write(root.path().join("docs/adr/0002-new.md"), "New\n").unwrap();
        git(root.path(), &["add", "."]);
        let violations = change_violations(
            &files,
            &Config::default(),
            Some(&worktree_changes(root.path())),
            Stage::Commit,
        );
        assert_eq!(violations.len(), 1);
        assert!(
            violations[0].starts_with("criv.toml at `HEAD` cannot be read"),
            "{violations:?}"
        );
    }

    #[test]
    fn scope_change_is_rejected_even_when_new_scope_hides_the_adr() {
        let root = decision_repository();
        let files = RepositoryFiles::open(root.path()).unwrap();
        fs::write(root.path().join("criv.toml"), "[vault]\ndocs = \"other\"\n").unwrap();
        fs::write(
            root.path().join("docs/adr/0001-base.md"),
            "Changed decision\n",
        )
        .unwrap();
        let current = Config::parse(Some("[vault]\ndocs = \"other\"\n")).unwrap();
        let violations = change_violations(
            &files,
            &current,
            Some(&worktree_changes(root.path())),
            Stage::Commit,
        );
        assert_eq!(
            violations,
            [
                "criv.toml moves the decision scope from `docs/adr` to `other/adr` in the same transaction as a decision change; the immutability gate would read the new scope"
            ]
        );
    }

    #[test]
    fn ci_allows_branch_local_deletion_but_rejects_a_published_edit() {
        let root = decision_repository();
        let files = RepositoryFiles::open(root.path()).unwrap();
        git(root.path(), &["checkout", "-b", "target"]);
        fs::write(
            root.path().join("docs/adr/0002-target.md"),
            "Target decision\n",
        )
        .unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-m", "target decision"]);
        git(root.path(), &["checkout", "main"]);
        fs::write(
            root.path().join("docs/adr/0001-base.md"),
            "Changed decision\n",
        )
        .unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-m", "edit published decision"]);
        let changes = GitRepository::open(root.path())
            .unwrap()
            .changed_set(
                &Comparison::Trees {
                    old_ref: "target",
                    new_ref: "HEAD",
                },
                &[],
            )
            .unwrap();
        let violations = change_violations(&files, &Config::default(), Some(&changes), Stage::Ci);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].starts_with("docs/adr/0001-base.md:"));
        assert_eq!(
            change_violations(&files, &Config::default(), Some(&changes), Stage::Push).len(),
            2
        );
    }

    #[test]
    fn immutability_allows_new_adrs_but_blocks_existing_changes() {
        let root = tempfile::TempDir::new().unwrap();
        let files = RepositoryFiles::open(root.path()).unwrap();
        let changes = changes(vec![
            entry(ChangeStatus::Added, "docs/adr/0012-new.md", None),
            entry(ChangeStatus::Modified, "docs/adr/0002-existing.md", None),
            entry(
                ChangeStatus::Renamed,
                "docs/adr/0003-renamed.md",
                Some("docs/adr/0003-existing.md"),
            ),
            entry(ChangeStatus::Modified, "docs/adr/README.md", None),
        ]);
        let violations =
            change_violations(&files, &Config::default(), Some(&changes), Stage::Commit);

        assert_eq!(violations.len(), 2);
        assert!(violations[0].contains("0002-existing"));
        assert!(violations[1].contains("0003-existing"));
    }

    #[test]
    fn working_tree_migration_requires_regular_readable_content() {
        let root = tempfile::TempDir::new().unwrap();
        git(root.path(), &["init"]);
        let path = "docs/adr/0001-test.md";
        fs::create_dir_all(root.path().join("docs/adr")).unwrap();
        fs::write(root.path().join(path), "See [[ADR-0010]].\n").unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-m", "add adr"]);
        let files = RepositoryFiles::open(root.path()).unwrap();
        let changes = changes(vec![entry(ChangeStatus::Modified, path, None)]);
        let permitted = || {
            change_violations(&files, &Config::default(), Some(&changes), Stage::Commit).is_empty()
        };
        let migrated = "See [[0010-example|ADR-0010]].\n";
        fs::write(root.path().join(path), migrated).unwrap();
        assert!(permitted());
        fs::remove_file(root.path().join(path)).unwrap();
        assert!(!permitted());
        #[cfg(unix)]
        {
            let external = tempfile::NamedTempFile::new().unwrap();
            fs::write(external.path(), migrated).unwrap();
            std::os::unix::fs::symlink(external.path(), root.path().join(path)).unwrap();
            assert!(!permitted());
        }
    }

    #[test]
    /// Accept equivalent ADR links with matching targets and fragments.
    fn portable_link_migrations_are_allowed() {
        let old = "See [[ADR-0010]] and [[ADR-0001#Context]].\n";
        let new = "See [[0010-criv-init-installs-agent-runtime-skills|ADR-0010]] and [[docs/adr/0001-local-cli-vault-architecture#Context|ADR-0001#Context]].\n";

        assert!(is_portable_link_migration(old, new));
    }

    #[test]
    fn portable_link_migrations_reject_content_edits() {
        let old = "See [[ADR-0010]].\n";
        let new = "Changed decision text and see [[0010-criv-init-installs-agent-runtime-skills|ADR-0010]].\n";

        assert!(!is_portable_link_migration(old, new));
    }

    #[test]
    fn portable_link_migrations_reject_mismatched_targets() {
        let old = "See [[ADR-0010]].\n";
        let new = "See [[0011-embed-runtime-skill-templates-as-assets|ADR-0010]].\n";

        assert!(!is_portable_link_migration(old, new));
    }
}
