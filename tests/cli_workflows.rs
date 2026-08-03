use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn criv(root: &Path) -> Command {
    let mut command = Command::cargo_bin("criv").expect("criv binary");
    command.current_dir(root);
    // Git exports these while running hooks. Every fixture owns its repository
    // context through `current_dir`, so inherited values must never redirect a
    // spawned CLI to the checkout that invoked the test suite.
    command.env_remove("GIT_DIR");
    command.env_remove("GIT_WORK_TREE");
    command.env_remove("GIT_INDEX_FILE");
    command.env_remove("GIT_COMMON_DIR");
    command.env_remove("GIT_PREFIX");
    command.env_remove("CI");
    command.env_remove("GITHUB_ACTIONS");
    command.env_remove("CRIV_BASE_REF");
    command.env_remove("GITHUB_BASE_REF");
    command
}

fn normalize_newlines(contents: &str) -> String {
    contents.replace("\r\n", "\n")
}

fn init(root: &Path) {
    criv(root)
        .args(["init", "--no-obsidian", "--no-skills"])
        .assert()
        .success();
}

fn init_with_skills(root: &Path) {
    criv(root)
        .args(["init", "--no-obsidian"])
        .assert()
        .success();
}

fn init_git_vault(root: &Path) {
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    init(root);
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial vault"]);
}

fn doc(id: &str, title: &str, body: &str) -> String {
    format!("---\nid: {id}\nkind: doc\ntitle: {title}\n---\n\n# {title}\n\n{body}\n")
}

#[test]
fn changed_check_fails_closed_outside_a_git_worktree() {
    let temp = TempDir::new().unwrap();

    criv(temp.path())
        .args(["check", "--changed"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("failed to open repository"));
}

#[test]
fn changed_check_with_no_staged_changes_skips_vault_loading_in_all_formats() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    git(root, &["init", "-b", "main"]);

    criv(root)
        .args(["check", "--changed"])
        .assert()
        .success()
        .stdout("criv check: ok\n");
    criv(root)
        .args(["check", "--changed", "--format", "json"])
        .assert()
        .success()
        .stdout("[]\n");
    criv(root)
        .args(["check", "--changed", "--format", "github"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn changed_check_validates_added_and_modified_notes_without_global_diagnostics() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    init_git_vault(root);
    fs::write(
        root.join("docs/unchanged-broken.md"),
        doc(
            "unchanged-broken",
            "Unchanged Broken",
            "See [[missing-global]].",
        ),
    )
    .unwrap();
    git(root, &["add", "docs/unchanged-broken.md"]);
    git(root, &["commit", "-m", "add known global defect"]);

    fs::write(
        root.join("docs/guide.md"),
        doc("guide", "Guide", "See [[missing-added]]."),
    )
    .unwrap();
    git(root, &["add", "docs/guide.md"]);
    criv(root)
        .args(["check", "--changed", "--filter", "broken-link"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("docs/guide.md"))
        .stdout(predicate::str::contains("docs/unchanged-broken.md").not());

    fs::write(
        root.join("docs/guide.md"),
        doc("guide", "Guide", "The added note is valid."),
    )
    .unwrap();
    git(root, &["add", "docs/guide.md"]);
    git(root, &["commit", "-m", "add valid guide"]);
    fs::write(
        root.join("docs/guide.md"),
        doc("guide", "Guide", "See [[missing-modified]]."),
    )
    .unwrap();
    git(root, &["add", "docs/guide.md"]);
    criv(root)
        .args(["check", "--changed", "--format", "json"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("missing-modified"))
        .stdout(predicate::str::contains("missing-global").not());
}

#[test]
fn changed_check_promotes_renames_and_deletions_to_full_validation() {
    for operation in ["rename", "delete"] {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        init_git_vault(root);
        fs::write(
            root.join("docs/unchanged-broken.md"),
            doc(
                "unchanged-broken",
                "Unchanged Broken",
                "See [[missing-global]].",
            ),
        )
        .unwrap();
        fs::write(
            root.join("docs/change-me.md"),
            doc("change-me", "Change Me", "Valid content."),
        )
        .unwrap();
        git(root, &["add", "docs"]);
        git(root, &["commit", "-m", "add validation fixtures"]);

        match operation {
            "rename" => git(root, &["mv", "docs/change-me.md", "docs/changed-name.md"]),
            "delete" => git(root, &["rm", "docs/change-me.md"]),
            _ => unreachable!(),
        }

        criv(root)
            .args(["check", "--changed", "--filter", "broken-link"])
            .assert()
            .failure()
            .stdout(predicate::str::contains("docs/unchanged-broken.md"));
    }
}

#[test]
fn changed_check_scans_policies_only_for_staged_sources() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    init_git_vault(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();
    fs::write(
        root.join("docs/adr/0999-no-println.md"),
        r#"---
id: ADR-0999
kind: decision
title: No println
status: accepted
date: 2026-08-03
governs:
  - src/**/*.rs
policy:
  patterns:
    - id: no-println
      language: rust
      pattern: "println!($$$ARGS)"
---

# No println
"#,
    )
    .unwrap();
    git(root, &["add", "src/lib.rs", "docs/adr/0999-no-println.md"]);
    git(root, &["commit", "-m", "add source policy"]);

    fs::write(
        root.join("src/lib.rs"),
        "pub fn run() {\n    println!(\"blocked\");\n}\n",
    )
    .unwrap();
    git(root, &["add", "src/lib.rs"]);

    criv(root)
        .args(["check", "--changed", "--filter", "policy-violation"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("ADR-0999 policy"))
        .stdout(predicate::str::contains("src/lib.rs:2"));
}

#[test]
fn changed_check_is_read_only() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    git(root, &["init", "-b", "main"]);

    criv(root)
        .args(["check", "--changed", "--fix"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn check_nudges_only_text_output_for_stale_skills() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    init_with_skills(root);
    let stale = root.join(".agents/skills/criv/SKILL.md");
    let contents = fs::read_to_string(&stale).unwrap();
    fs::write(
        &stale,
        contents.replace("criv-template: blake3:", "criv-template: blake3:stale-"),
    )
    .unwrap();

    criv(root)
        .arg("check")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "note: 1 agent skill is out of date; run `criv init --force-skills`",
        ));
    criv(root)
        .args(["check", "--filter", "does-not-match"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "note: 1 agent skill is out of date; run `criv init --force-skills`",
        ));

    let json = criv(root)
        .args(["check", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice::<Vec<serde_json::Value>>(&json).unwrap();
    criv(root)
        .args(["check", "--format", "github"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn check_is_silent_about_deliberately_absent_skills() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    criv(root)
        .args(["init", "--no-obsidian", "--no-vscode", "--no-skills"])
        .assert()
        .success();
    criv(root)
        .arg("check")
        .assert()
        .success()
        .stdout(predicate::str::contains("out of date").not());
}

#[test]
fn force_skills_cli_refresh_creates_only_skills() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    criv(root)
        .args(["init", "--no-obsidian", "--no-vscode", "--no-skills"])
        .assert()
        .success();

    criv(root)
        .args(["init", "--force-skills"])
        .assert()
        .success();

    assert!(root.join(".agents/skills/criv/SKILL.md").exists());
    assert!(root.join(".claude/skills/criv/SKILL.md").exists());
    assert!(!root.join(".obsidian").exists());
    assert!(!root.join(".vscode/extensions.json").exists());
    assert!(!root.join(".githooks").exists());
}

#[test]
fn unreadable_skill_content_never_breaks_check_or_machine_formats() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    init_with_skills(root);
    fs::write(root.join(".agents/skills/criv/SKILL.md"), [0xff, 0xfe]).unwrap();

    criv(root).arg("check").assert().success();
    let json = criv(root)
        .args(["check", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice::<Vec<serde_json::Value>>(&json).unwrap();
    criv(root)
        .args(["check", "--format", "github"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn init_check_watch_query_search_and_enforce_workflow() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);

    criv(root).arg("check").assert().success();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn run() -> &'static str {\n    \"criv\"\n}\n",
    )
    .unwrap();

    criv(root).args(["watch", "--once"]).assert().success();

    assert!(root.join(".criv/state.json").exists());
    assert!(root.join(".criv/latest").exists());
    assert!(
        fs::read_dir(root.join(".criv/snapshots"))
            .unwrap()
            .next()
            .is_some()
    );

    criv(root).args(["query", "coverage"]).assert().success();
    criv(root)
        .args(["query", "nodes", "--kind", "code", "--without-docs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/lib.rs#fn:run"));
    criv(root)
        .args(["search", "--files", "lib"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/lib.rs"));
    criv(root)
        .args(["search", "--grep", "criv"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/lib.rs:2"));
    criv(root)
        .args(["enforce", "--stage", "ci"])
        .assert()
        .success();
}

#[test]
fn source_graph_cache_is_written_and_refreshed() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();

    criv(root).args(["watch", "--once"]).assert().success();

    let cache_path = root.join(".criv/source-graph.json");
    assert!(cache_path.exists());
    let before = fs::read_to_string(&cache_path).unwrap();
    let before: serde_json::Value = serde_json::from_str(&before).unwrap();
    let before_fingerprint = before["graph"]["file_fingerprints"]["src/lib.rs"]
        .as_str()
        .unwrap()
        .to_string();

    fs::write(
        root.join("src/lib.rs"),
        "pub fn run() {}\n\npub fn added() {}\n",
    )
    .unwrap();
    criv(root).arg("check").assert().success();

    let after = fs::read_to_string(&cache_path).unwrap();
    let after: serde_json::Value = serde_json::from_str(&after).unwrap();
    assert_eq!(after["schema"], "criv.source-graph/2");
    assert_ne!(
        after["graph"]["file_fingerprints"]["src/lib.rs"]
            .as_str()
            .unwrap(),
        before_fingerprint
    );
}

#[test]
fn consecutive_watch_once_runs_produce_the_same_snapshot() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    write_criv_config(root, vec!["src"], vec![], true);

    // The second run reuses the on-disk source graph cache; reuse must not
    // change what the run produces.
    let first = criv(root).args(["watch", "--once"]).assert().success();
    let second = criv(root).args(["watch", "--once"]).assert().success();

    let snapshot = |assert: &assert_cmd::assert::Assert| {
        String::from_utf8(assert.get_output().stdout.clone()).unwrap()
    };
    assert_eq!(snapshot(&first), snapshot(&second));
}

#[test]
fn stale_effective_governance_fails_all_check_formats_and_preserves_publication() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs/adr")).unwrap();
    fs::write(root.join("src/retired.rs"), "pub fn retired() {}\n").unwrap();
    fs::write(
        root.join("docs/adr/0999-retired.md"),
        r#"---
id: ADR-0999
kind: decision
title: Retired
status: accepted
governs:
  - src/retired.rs
---

# Retired
"#,
    )
    .unwrap();
    criv(root).args(["watch", "--once"]).assert().success();
    let state_before = fs::read(root.join(".criv/state.json")).unwrap();
    let latest_before = fs::read(root.join(".criv/latest")).unwrap();
    let mut snapshots_before = fs::read_dir(root.join(".criv/snapshots"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    snapshots_before.sort();

    fs::remove_file(root.join("src/retired.rs")).unwrap();

    criv(root)
        .args(["check", "--filter", "unresolved-governs"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("error[unresolved-governs]"))
        .stdout(predicate::str::contains("src/retired.rs"));
    let json = criv(root)
        .args([
            "check",
            "--format",
            "json",
            "--filter",
            "unresolved-governs",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let diagnostics: Vec<serde_json::Value> = serde_json::from_slice(&json).unwrap();
    assert_eq!(diagnostics[0]["severity"], "error");
    assert_eq!(diagnostics[0]["code"], "unresolved-governs");
    criv(root)
        .args([
            "check",
            "--format",
            "github",
            "--filter",
            "unresolved-governs",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            "::error file=docs/adr/0999-retired.md,title=criv unresolved-governs::",
        ));
    criv(root)
        .args(["watch", "--once"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("state publication blocked"))
        .stderr(predicate::str::contains("src/retired.rs"));

    assert_eq!(
        fs::read(root.join(".criv/state.json")).unwrap(),
        state_before
    );
    assert_eq!(fs::read(root.join(".criv/latest")).unwrap(), latest_before);
    let mut snapshots_after = fs::read_dir(root.join(".criv/snapshots"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    snapshots_after.sort();
    assert_eq!(snapshots_after, snapshots_before);
}

#[test]
fn watch_once_does_not_rebuild_while_watch_lock_is_held() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();

    criv(root).args(["watch", "--once"]).assert().success();
    let state_before = fs::read_to_string(root.join(".criv/state.json")).unwrap();
    // The test process is genuinely alive, so this lock must be honored.
    fs::write(root.join(".criv/watch.lock"), live_pid_lock()).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn changed() {}\n").unwrap();

    criv(root)
        .args(["watch", "--once"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "active watcher already owns state refresh",
        ))
        .stderr(predicate::str::contains("watch --once"));

    let state_after = fs::read_to_string(root.join(".criv/state.json")).unwrap();
    assert_eq!(state_after, state_before);
}

#[test]
fn watch_once_reclaims_a_lock_left_behind_by_a_dead_watcher() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();

    criv(root).args(["watch", "--once"]).assert().success();
    let state_before = fs::read_to_string(root.join(".criv/state.json")).unwrap();

    // A watcher that crashed leaves its lock behind; the PID it recorded is not
    // running, so the next run must reclaim it instead of failing forever.
    fs::write(root.join(".criv/watch.lock"), dead_pid_lock()).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn changed() {}\n").unwrap();

    criv(root).args(["watch", "--once"]).assert().success();

    let state_after = fs::read_to_string(root.join(".criv/state.json")).unwrap();
    assert_ne!(state_after, state_before);
    assert!(!root.join(".criv/watch.lock").exists());
}

#[test]
fn watch_once_reclaims_a_malformed_lock() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();

    // A lock written by an older criv carries no owner; it must not wedge the
    // repository permanently.
    fs::create_dir_all(root.join(".criv")).unwrap();
    fs::write(root.join(".criv/watch.lock"), "active").unwrap();

    criv(root).args(["watch", "--once"]).assert().success();

    assert!(!root.join(".criv/watch.lock").exists());
}

/// A lock owned by this still-running test process.
fn live_pid_lock() -> String {
    format!("pid {}\nstart \n", std::process::id())
}

/// A PID that has exited: spawn a trivial process and wait for it, so the number
/// is real and reachable-but-gone rather than an arbitrary guess.
fn dead_pid_lock() -> String {
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    format!("pid {pid}\nstart Mon Jan  1 00:00:00 2001\n")
}

#[test]
fn disabled_source_index_is_observed_through_cli_boundary() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();
    write_criv_config(
        root,
        vec!["src"],
        vec!["**/target/**", "**/node_modules/**"],
        false,
    );

    criv(root).args(["watch", "--once"]).assert().success();

    let state = fs::read_to_string(root.join(".criv/state.json")).unwrap();
    let state: serde_json::Value = serde_json::from_str(&state).unwrap();
    assert!(state["source-index"].as_array().unwrap().is_empty());

    criv(root)
        .args(["search", "--files", "lib"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
    criv(root)
        .args(["query", "nodes", "--kind", "code", "--without-docs"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn architecture_code_output_cannot_escape_vault_root() {
    let parent = TempDir::new().unwrap();
    let root = parent.path().join("vault");
    fs::create_dir_all(&root).unwrap();

    init(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();
    fs::write(
        root.join("criv.toml"),
        r#"[vault]
docs = "docs"
adr = "adr"

[source]
roots = ["src"]
exclude = []

[index]
source = true
embeddings = false

[enforce]
stages = ["commit", "push", "ci"]

[architecture.code]
output = "../outside.c4"
"#,
    )
    .unwrap();

    criv(&root)
        .args(["watch", "--once"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("architecture.code.output"))
        .stderr(predicate::str::contains("parent-directory"));
    assert!(!parent.path().join("outside.c4").exists());
}

#[test]
fn check_accepts_valid_crlf_frontmatter() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    init(root);
    fs::write(
        root.join("docs/crlf.md"),
        "---\r\nid: crlf\r\nkind: doc\r\ntitle: CRLF note\r\n---\r\n\r\n## CRLF note\r\n",
    )
    .unwrap();

    criv(root).arg("check").assert().success();
}

#[test]
fn file_search_honors_path_and_language_filters() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("scripts/main.py"), "def main():\n    pass\n").unwrap();
    write_criv_config(
        root,
        vec!["src", "scripts"],
        vec!["**/target/**", "**/node_modules/**"],
        true,
    );

    criv(root)
        .args(["search", "--files", "main"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/main.rs"))
        .stdout(predicate::str::contains("scripts/main.py"));
    criv(root)
        .args(["search", "--files", "main", "--paths", "src/**"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/main.rs"))
        .stdout(predicate::str::contains("scripts/main.py").not());
    criv(root)
        .args(["search", "--files", "main", "--lang", "rust"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/main.rs"))
        .stdout(predicate::str::contains("scripts/main.py").not());
}

#[test]
fn structural_search_without_a_path_filter_scans_every_source_file() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("scripts/tool.rs"), "fn tool() {}\n").unwrap();
    write_criv_config(root, vec!["src", "scripts"], vec![], true);

    // No `--paths` and no `--lang`: the scan must cover the whole source index
    // rather than compiling an empty glob set that matches nothing.
    criv(root)
        .args(["search", "fn $NAME() { $$$ }"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/main.rs"))
        .stdout(predicate::str::contains("scripts/tool.rs"));

    criv(root)
        .args(["search", "fn $NAME() { $$$ }", "--paths", "src/**"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/main.rs"))
        .stdout(predicate::str::contains("scripts/tool.rs").not());
}

#[test]
fn file_search_matches_jsx_for_the_jsx_language_filter() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/component.jsx"),
        "export const Component = () => <div />;\n",
    )
    .unwrap();
    fs::write(
        root.join("src/component.js"),
        "export const component = 1;\n",
    )
    .unwrap();
    write_criv_config(root, vec!["src"], vec![], true);

    criv(root)
        .args(["search", "--files", "component", "--lang", "jsx"])
        .assert()
        .success()
        .stdout("src/component.jsx\n");
}

// Windows cannot represent the quote, tab, and newline characters used in the
// filename fixture. JSON escaping remains covered there by the search test.
#[cfg(not(windows))]
#[test]
fn query_json_output_is_valid_for_special_characters() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    let special_file = "quoted\"\tline\nbreak.rs";
    fs::write(root.join("src").join(special_file), "pub fn run() {}\n").unwrap();
    write_criv_config(
        root,
        vec!["src"],
        vec!["**/target/**", "**/node_modules/**"],
        true,
    );

    let assert = criv(root)
        .args(["query", "nodes", "--kind", "code", "--format", "json"])
        .assert()
        .success();
    let rows: Vec<String> = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert!(rows.iter().any(|row| {
        row == &format!("src/{special_file}#fn:run")
            && row.contains('"')
            && row.contains('\t')
            && row.contains('\n')
    }));
}

#[test]
fn query_subcommands_cover_docs_sources_and_governance() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    query_fixture(root);

    criv(root)
        .args(["query", "next-adr-id"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ADR-0002"));
    criv(root)
        .args(["query", "callers", "src/lib.rs#fn:helper"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/lib.rs#fn:run"));
    criv(root)
        .args(["query", "callees", "src/lib.rs#fn:run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/lib.rs#fn:helper"));
    criv(root)
        .args(["query", "attack-surface"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/lib.rs#fn:run"));
    criv(root)
        .args(["query", "targets", "guide"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/lib.rs"));
    criv(root)
        .args(["query", "cites", "guide"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ADR-0001"))
        .stdout(predicate::str::contains("src/lib.rs"));
    criv(root)
        .args(["query", "cited-by", "ADR-0001"])
        .assert()
        .success()
        .stdout(predicate::str::contains("guide"));
    criv(root)
        .args(["query", "governs", "ADR-0001"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/lib.rs"));
    criv(root)
        .args(["query", "governing", "src/lib.rs#fn:run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ADR-0001"));
    criv(root)
        .args(["query", "references", "src/lib.rs#fn:run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("guide"));
    criv(root)
        .args(["query", "orphan-docs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("orphan"))
        .stdout(predicate::str::contains("guide").not());

    let callers = criv(root)
        .args([
            "query",
            "callers",
            "src/lib.rs#fn:helper",
            "--format",
            "json",
        ])
        .assert()
        .success();
    let callers: Vec<String> = serde_json::from_slice(&callers.get_output().stdout).unwrap();
    assert!(callers.iter().any(|row| row == "src/lib.rs#fn:run"));

    let governs = criv(root)
        .args(["query", "governs", "ADR-0001", "--format", "json"])
        .assert()
        .success();
    let governs: Vec<String> = serde_json::from_slice(&governs.get_output().stdout).unwrap();
    assert!(governs.iter().any(|row| row == "src/lib.rs"));
}

#[test]
fn snapshot_and_docs_queries_do_not_touch_the_source_graph_cache() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    query_fixture(root);
    let cache_path = root.join(".criv/source-graph.json");
    let original_cache = fs::read(&cache_path).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn run() {\n    helper();\n}\n\nfn helper() {}\n\npub fn added() {}\n",
    )
    .unwrap();

    let config_path = root.join("criv.toml");
    let valid_config = fs::read_to_string(&config_path).unwrap();
    fs::write(&config_path, "[source\n").unwrap();
    criv(root)
        .args(["query", "diff", "latest", "latest"])
        .assert()
        .success();
    assert_eq!(fs::read(&cache_path).unwrap(), original_cache);
    fs::write(&config_path, valid_config).unwrap();

    let docs_queries = [
        vec!["query", "next-adr-id"],
        vec!["query", "cited-by", "ADR-0001"],
        vec!["query", "orphan-docs"],
        vec!["query", "nodes", "--kind", "doc"],
        vec!["query", "nodes", "--kind", "decision"],
    ];
    for args in &docs_queries {
        criv(root).args(args).assert().success();
        assert_eq!(fs::read(&cache_path).unwrap(), original_cache, "{args:?}");
    }

    fs::remove_file(&cache_path).unwrap();
    criv(root)
        .args(["query", "diff", "latest", "latest"])
        .assert()
        .success();
    assert!(!cache_path.exists());
    for args in &docs_queries {
        criv(root).args(args).assert().success();
        assert!(!cache_path.exists(), "{args:?}");
    }

    criv(root)
        .args(["query", "attack-surface"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/lib.rs#fn:added"));
    assert!(cache_path.exists());
    assert_ne!(fs::read(cache_path).unwrap(), original_cache);
}

#[test]
fn query_usage_errors_are_reported() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    query_fixture(root);

    criv(root)
        .args(["query", "callers"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("required arguments"))
        .stderr(predicate::str::contains("<SYMBOL>"));
    let invalid = criv(root)
        .args(["query", "bogus"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("unrecognized subcommand 'bogus'"))
        .stderr(predicate::str::contains("Valid query subcommands:"))
        .stderr(predicate::str::contains("MVP").not());
    let stderr = String::from_utf8(invalid.get_output().stderr.clone()).unwrap();
    for name in [
        "next-adr-id",
        "callers",
        "callees",
        "attack-surface",
        "targets",
        "cites",
        "cited-by",
        "orphan-docs",
        "references",
        "governs",
        "governing",
        "coverage",
        "nodes",
        "c4-elements",
        "c4-relationships",
        "c4-code",
        "diff",
    ] {
        assert!(stderr.contains(name), "missing query subcommand {name}");
    }

    criv(root)
        .args(["query", "diff", "latest"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("required arguments"))
        .stderr(predicate::str::contains("<REF_B>"));
    criv(root)
        .args(["query", "coverage", "--kind", "code"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("unexpected argument '--kind'"));
    criv(root)
        .args(["query", "callers", "src/lib.rs", "--without-docs"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains(
            "unexpected argument '--without-docs'",
        ));
    criv(root)
        .args(["query", "coverage", "--by", "invalid"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("invalid value 'invalid'"))
        .stderr(predicate::str::contains("module, adr"));
    criv(root)
        .args(["query", "nodes", "--kind", "invalid"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("invalid value 'invalid'"))
        .stderr(predicate::str::contains("code, doc, decision"));
    criv(root)
        .args(["query", "coverage", "extra"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("unexpected argument 'extra'"));
}

#[test]
fn query_diff_compares_snapshots_and_reports_errors() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    query_fixture(root);
    let hash_a = fs::read_to_string(root.join(".criv/latest"))
        .unwrap()
        .trim()
        .to_string();

    criv(root)
        .args(["query", "diff", "latest", "latest"])
        .assert()
        .success()
        .stdout(predicate::str::contains("node_added").not())
        .stdout(predicate::str::contains("node_removed").not());

    fs::write(
        root.join("src/lib.rs"),
        "pub fn run() {\n    helper();\n}\n\nfn helper() {}\n\npub fn added() {}\n",
    )
    .unwrap();
    criv(root).args(["watch", "--once"]).assert().success();
    let hash_b = fs::read_to_string(root.join(".criv/latest"))
        .unwrap()
        .trim()
        .to_string();
    assert_ne!(hash_a, hash_b);

    criv(root)
        .args(["query", "diff", &hash_a, &hash_b])
        .assert()
        .success()
        .stdout(predicate::str::contains("node_added"))
        .stdout(predicate::str::contains("src/lib.rs#fn:added"));
    criv(root)
        .args(["query", "diff", &hash_b, &hash_a])
        .assert()
        .success()
        .stdout(predicate::str::contains("node_removed"))
        .stdout(predicate::str::contains("src/lib.rs#fn:added"));
    criv(root)
        .args(["query", "diff", &hash_a])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("required arguments"))
        .stderr(predicate::str::contains("<REF_B>"));
    criv(root)
        .args(["query", "diff", "nonexistent", "latest"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not resolve"));
}

#[test]
fn state_commands_bound_snapshots_and_preserve_git_ref_diffing() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    query_fixture(root);
    let original_hash = fs::read_to_string(root.join(".criv/latest"))
        .unwrap()
        .trim()
        .to_string();

    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    git(root, &["add", "-f", ".criv/state.json"]);
    git(root, &["commit", "-m", "record durable state"]);

    let mut config = fs::read_to_string(root.join("criv.toml")).unwrap();
    config.push_str("\n[state]\nkeep = 2\n");
    fs::write(root.join("criv.toml"), config).unwrap();
    for source in [
        "pub fn run() {}\npub fn second() {}\n",
        "pub fn run() {}\npub fn third() {}\n",
    ] {
        fs::write(root.join("src/lib.rs"), source).unwrap();
        criv(root).args(["watch", "--once"]).assert().success();
    }

    let listed = criv(root)
        .args(["state", "list", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let listed: Vec<serde_json::Value> = serde_json::from_slice(&listed).unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0]["position"], 1);
    assert_eq!(listed[0]["latest"], true);
    assert!(listed.iter().all(|record| record["hash"] != original_hash));

    let preview = criv(root)
        .args([
            "state",
            "prune",
            "--keep",
            "1",
            "--dry-run",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let preview: serde_json::Value = serde_json::from_slice(&preview).unwrap();
    assert_eq!(preview["removed"].as_array().unwrap().len(), 1);
    assert_eq!(preview["dry_run"], true);
    criv(root)
        .args(["state", "list", "--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"position\":2"));

    criv(root)
        .args(["state", "prune", "--keep", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed=1"))
        .stdout(predicate::str::contains("retained=1"));
    let after = criv(root)
        .args(["state", "list", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        serde_json::from_slice::<Vec<serde_json::Value>>(&after)
            .unwrap()
            .len(),
        1
    );

    criv(root)
        .args(["query", "diff", "HEAD", "HEAD"])
        .assert()
        .success();
    criv(root)
        .args(["state", "prune", "--keep", "0"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("invalid value '0'"));
}

#[test]
fn query_diff_reads_state_from_a_git_ref() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    query_fixture(root);
    git(root, &["init"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    git(root, &["add", "-f", ".criv/state.json"]);
    git(root, &["commit", "-m", "record state"]);

    criv(root)
        .args(["query", "diff", "HEAD", "HEAD"])
        .assert()
        .success()
        .stdout(predicate::str::contains("node_added").not())
        .stdout(predicate::str::contains("node_removed").not());
}

#[test]
fn query_diff_uses_the_requested_root_despite_inherited_git_context() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    query_fixture(root);
    git(root, &["init"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    git(root, &["add", "-f", ".criv/state.json"]);
    git(root, &["commit", "-m", "record state"]);

    let outer = TempDir::new().unwrap();
    git(outer.path(), &["init"]);

    criv(root)
        .env("GIT_DIR", outer.path().join(".git"))
        .env("GIT_WORK_TREE", outer.path())
        .args(["query", "diff", "HEAD", "HEAD"])
        .assert()
        .success();
}

#[test]
fn query_diff_reads_a_git_ref_without_a_git_executable() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    query_fixture(root);
    git(root, &["init"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    git(root, &["add", "-f", ".criv/state.json"]);
    git(root, &["commit", "-m", "record state"]);

    let empty_path = TempDir::new().unwrap();
    criv(root)
        .env("PATH", empty_path.path())
        .args(["query", "diff", "HEAD", "HEAD"])
        .assert()
        .success()
        .stdout(predicate::str::contains("node_added").not())
        .stdout(predicate::str::contains("node_removed").not());
}

#[test]
fn query_diff_reads_a_git_ref_from_a_linked_worktree() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    query_fixture(root);
    git(root, &["init"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    git(root, &["add", "criv.toml", "src", "docs"]);
    git(root, &["add", "-f", ".criv/state.json"]);
    git(root, &["commit", "-m", "record vault and state"]);

    let linked = root.join("linked-worktree");
    git(root, &["worktree", "add", linked.to_str().unwrap(), "HEAD"]);

    criv(&linked)
        .env("PATH", TempDir::new().unwrap().path())
        .args(["query", "diff", "HEAD", "HEAD"])
        .assert()
        .success();
}

#[test]
fn query_diff_rejects_non_utf8_state_from_a_git_ref() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    query_fixture(root);
    git(root, &["init"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    fs::write(root.join(".criv/state.json"), [0xff, 0xfe]).unwrap();
    git(root, &["add", "-f", ".criv/state.json"]);
    git(root, &["commit", "-m", "record invalid state"]);

    criv(root)
        .args(["query", "diff", "HEAD", "HEAD"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("non-UTF-8 .criv/state.json"));
}

#[test]
fn search_json_output_is_valid_for_special_characters() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn run() { let text = \"quoted\tvalue\"; }\n",
    )
    .unwrap();
    write_criv_config(
        root,
        vec!["src"],
        vec!["**/target/**", "**/node_modules/**"],
        true,
    );

    let assert = criv(root)
        .args(["search", "--grep", "quoted", "--format", "json"])
        .assert()
        .success();
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    let row = rows
        .iter()
        .find(|row| row["path"] == "src/lib.rs")
        .expect("grep row");
    assert_eq!(row["line"], 1);
    assert_eq!(
        row["text"],
        "pub fn run() { let text = \"quoted\tvalue\"; }"
    );
}

#[test]
fn regex_grep_reports_invalid_queries() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();
    write_criv_config(
        root,
        vec!["src"],
        vec!["**/target/**", "**/node_modules/**"],
        true,
    );

    criv(root)
        .args(["search", "--grep", "[", "--grep-mode", "regex"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid regex grep query"));
}

#[test]
fn check_json_output_is_valid_for_special_characters() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::write(
        root.join("docs/broken.md"),
        r#"---
id: broken
kind: doc
title: Broken
---

# Broken

Missing [[missing"	target
file.rs]]
"#,
    )
    .unwrap();

    let assert = criv(root)
        .args(["check", "--format", "json", "--filter", "broken-link"])
        .assert()
        .failure();
    let diagnostics: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).unwrap();
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic["code"] == "broken-link")
        .expect("broken-link diagnostic");
    assert_eq!(diagnostic["severity"], "error");
    assert_eq!(diagnostic["path"], "docs/broken.md");
    assert!(
        diagnostic["message"]
            .as_str()
            .unwrap()
            .contains("\"	target\n")
    );
}

#[test]
fn check_github_output_emits_workflow_annotation() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::write(
        root.join("docs/broken.md"),
        r#"---
id: broken
kind: doc
title: Broken
---

# Broken

Missing [[missing-target]]
"#,
    )
    .unwrap();

    criv(root)
        .args(["check", "--format", "github", "--filter", "broken-link"])
        .assert()
        .failure()
        .stdout(
            "::error file=docs/broken.md,line=9,title=criv broken-link::wiki-link `[[missing-target]]` does not resolve\n",
        );
}

#[test]
fn check_fix_rewrites_fixable_markdown_in_the_docs_tree() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    let messy = root.join("docs/messy.md");
    fs::write(
        &messy,
        "---\nid: messy\nkind: doc\ntitle: Messy\n---\n\n## Messy\n\nTrailing spaces here.   \n\n\n\nToo many blank lines above.\n",
    )
    .unwrap();

    criv(root).args(["check", "--fix"]).assert().success();

    let fixed = fs::read_to_string(&messy).unwrap();
    assert!(
        !fixed.contains("here.   \n"),
        "trailing whitespace should be fixed: {fixed:?}"
    );
    assert!(
        !fixed.contains("\n\n\n"),
        "consecutive blank lines should be collapsed: {fixed:?}"
    );
}

#[test]
fn check_fix_leaves_already_clean_markdown_untouched() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    let clean = root.join("docs/clean.md");
    let contents = "---\nid: clean\nkind: doc\ntitle: Clean\n---\n\n## Clean\n\nBody text.\n";
    fs::write(&clean, contents).unwrap();

    criv(root).args(["check", "--fix"]).assert().success();

    assert_eq!(
        fs::read_to_string(&clean).unwrap(),
        contents,
        "a file with nothing to fix must not be rewritten"
    );
}

#[test]
fn check_fix_rewrites_markdown_outside_the_docs_tree() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    let readme = root.join("README.md");
    fs::write(&readme, "# Root\n\nTrailing spaces here.   \n").unwrap();

    criv(root).args(["check", "--fix"]).assert().success();

    assert_eq!(
        fs::read_to_string(&readme).unwrap(),
        "# Root\n\nTrailing spaces here.\n",
        "ADR-0044: --fix rewrites every Markdown file check lints inside the root"
    );
}

#[test]
fn check_fix_honors_the_rumdl_exclude_list() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::write(
        root.join(".rumdl.toml"),
        "[global]\nexclude = [\"vendor/**\"]\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("vendor")).unwrap();
    let vendored = root.join("vendor/upstream.md");
    let contents = "# Upstream\n\nTrailing spaces here.   \n";
    fs::write(&vendored, contents).unwrap();

    criv(root).args(["check", "--fix"]).assert().success();

    assert_eq!(
        fs::read_to_string(&vendored).unwrap(),
        contents,
        "the rumdl exclude list is the scope control for --fix"
    );
}

#[test]
fn check_fix_refuses_to_write_outside_the_repository_root() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let outside = TempDir::new().unwrap();

    init(root);
    let target = outside.path().join("secret.md");
    let contents = "# Secret\n\nTrailing spaces here.   \n";
    fs::write(&target, contents).unwrap();

    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, root.join("ESCAPE.md")).unwrap();

    criv(root).args(["check", "--fix"]).assert().success();

    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        contents,
        "no --fix write may reach a file outside the repository root"
    );
}

#[test]
fn generated_plugin_bundle_is_excluded_from_source_graph() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join(".obsidian/plugins/criv/src")).unwrap();
    fs::create_dir_all(root.join(".obsidian/plugins/criv/pkg")).unwrap();
    fs::write(
        root.join(".obsidian/plugins/criv/src/main.ts"),
        "export function activate(): string {\n  return \"criv\";\n}\n",
    )
    .unwrap();
    fs::write(
        root.join(".obsidian/plugins/criv/main.js"),
        "function bundledGeneratedSymbol() { return 'generated'; }\n",
    )
    .unwrap();
    fs::write(
        root.join(".obsidian/plugins/criv/pkg/criv_wasm.js"),
        "export function generatedWasmHelper() {}\n",
    )
    .unwrap();
    write_criv_config(
        root,
        vec![".obsidian/plugins/criv"],
        vec![
            ".obsidian/plugins/criv/main.js",
            ".obsidian/plugins/criv/pkg/**",
        ],
        true,
    );

    criv(root)
        .args(["query", "nodes", "--kind", "code", "--without-docs"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            ".obsidian/plugins/criv/src/main.ts#fn:activate",
        ))
        .stdout(predicate::str::contains("bundledGeneratedSymbol").not())
        .stdout(predicate::str::contains("generatedWasmHelper").not());
}

#[test]
fn source_fragment_validation_checks_symbols_and_line_links() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs/adr")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();
    write_criv_config(
        root,
        vec!["src"],
        vec!["**/target/**", "**/node_modules/**"],
        true,
    );
    fs::write(
        root.join("docs/adr/0999-source-fragments.md"),
        r#"---
id: ADR-0999
kind: decision
title: Source fragments
status: accepted
targets:
  symbols:
    - src/lib.rs#missing
---

# Source fragments

Missing [[src/lib.rs#absent]]
"#,
    )
    .unwrap();

    criv(root)
        .arg("check")
        .assert()
        .failure()
        .stdout(predicate::str::contains("unresolved-target"))
        .stdout(predicate::str::contains("source symbol"))
        .stdout(predicate::str::contains("broken-link"))
        .stdout(predicate::str::contains("src/lib.rs#absent"));

    fs::write(
        root.join("docs/adr/0999-source-fragments.md"),
        r#"---
id: ADR-0999
kind: decision
title: Source fragments
status: accepted
targets:
  symbols:
    - src/lib.rs#run
    - src/lib.rs#L1
    - src/lib.rs#L1-L1
---

# Source fragments

Line [[src/lib.rs#L1]]
Range [[src/lib.rs#L1-L1]]
"#,
    )
    .unwrap();

    criv(root)
        .args(["check", "--filter", "unresolved-target"])
        .assert()
        .success();
    criv(root)
        .args(["check", "--filter", "broken-link"])
        .assert()
        .success();
}

#[test]
fn c4_standard_alignment_cli_smoke_test() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn run() {\n    helper();\n}\n\nfn helper() {}\n",
    )
    .unwrap();
    write_criv_config(
        root,
        vec!["src"],
        vec!["**/target/**", "**/node_modules/**"],
        true,
    );

    fs::write(
        root.join("docs/c4.md"),
        r#"---
id: c4
kind: doc
title: C4 Smoke
---

## C4 Smoke

```mermaid
C4Container
System_Boundary(system, "criv") {
    Container(cli, "criv CLI", "Rust", "Runs local validation")
    Container(plugin, "Obsidian Plugin", "TypeScript", "Reads generated state")
}
Rel(cli, plugin, "writes state for")
```
"#,
    )
    .unwrap();

    criv(root).arg("check").assert().success();
    criv(root)
        .args(["query", "c4-elements", "c4"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "alias=cli category=container kind=Container",
        ))
        .stdout(predicate::str::contains(
            "alias=plugin category=container kind=Container",
        ))
        .stdout(predicate::str::contains("alias=system").not());
    criv(root)
        .args(["query", "c4-relationships", "c4"])
        .assert()
        .success()
        .stdout(predicate::str::contains("label=writes state for"));
    criv(root)
        .args(["query", "c4-code", "src/lib.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("classDiagram"))
        .stdout(predicate::str::contains("class run"))
        .stdout(predicate::str::contains("run --> helper"));

    fs::write(
        root.join("docs/c4.md"),
        r#"---
id: c4
kind: doc
title: C4 Smoke
---

## C4 Smoke

```mermaid
C4Container
System_Boundary(system, "criv") {
    Container(cli, "criv CLI", "Rust", "Runs local validation")
}
Rel(cli, system, "runs inside")
```
"#,
    )
    .unwrap();
    criv(root)
        .args(["check", "--filter", "unresolved-c4-relationship"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("unresolved-c4-relationship"));

    fs::write(
        root.join("docs/c4.md"),
        r#"---
id: c4
kind: doc
title: C4 Smoke
---

## C4 Smoke

```mermaid
C4Container
Component(parser, "C4 Parser", "Rust", "Parses Mermaid C4 blocks")
```
"#,
    )
    .unwrap();
    criv(root)
        .args(["check", "--filter", "invalid-c4-level"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("invalid-c4-level"));
}

#[test]
fn unresolved_pattern_diagnostic_uses_file_line_number() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs/adr")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();
    write_criv_config(
        root,
        vec!["src"],
        vec!["**/target/**", "**/node_modules/**"],
        true,
    );
    fs::write(
        root.join("docs/adr/0998-pattern-lines.md"),
        r#"---
id: ADR-0998
kind: decision
title: Pattern lines
status: accepted
targets:
  patterns:
    - { ref: missing/pattern }
---

# Pattern lines
"#,
    )
    .unwrap();

    criv(root)
        .args(["check", "--filter", "unresolved-pattern"])
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            "docs/adr/0998-pattern-lines.md:8:",
        ))
        .stdout(predicate::str::contains(
            "pattern reference `missing/pattern` does not resolve",
        ));
}

#[test]
fn selector_scoped_policy_patterns_are_enforced() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs/adr")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn run() {\n    println!(\"blocked\");\n}\n",
    )
    .unwrap();
    write_criv_config(
        root,
        vec!["src"],
        vec!["**/target/**", "**/node_modules/**"],
        true,
    );
    fs::write(
        root.join("docs/adr/0997-selector-policy.md"),
        r#"---
id: ADR-0997
kind: decision
title: Selector policy
status: accepted
governs:
  - src/lib.rs#fn:run
policy:
  patterns:
    - id: no-println
      language: rust
      pattern: "println!($$$ARGS)"
---

# Selector policy
"#,
    )
    .unwrap();

    criv(root)
        .args(["check", "--filter", "policy-violation"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("ADR-0997 policy"))
        .stdout(predicate::str::contains("println!"));
}

#[test]
fn inline_policy_pattern_definitions_are_validated() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("docs/adr")).unwrap();
    write_criv_config(
        root,
        vec!["src"],
        vec!["**/target/**", "**/node_modules/**"],
        false,
    );
    fs::write(
        root.join("docs/adr/0996-inline-policy-validation.md"),
        r#"---
id: ADR-0996
kind: decision
title: Inline policy validation
status: accepted
date: 2026-06-26
policy:
  patterns:
    - language: rust
      pattern: "println!($$$ARGS)"
    - id: ""
      language: rust
      pattern: "println!($$$ARGS)"
    - id: no-language
      pattern: "println!($$$ARGS)"
    - id: invalid-rule
      language: rust
      rule: "not: [valid"
    - id: id-only
    - id: ambiguous
      language: rust
      pattern: "println!($$$ARGS)"
      rule: "pattern: println!($$$ARGS)"
    - id: no-body
      language: rust
      message: Missing a body.
    - id: unsupported-language
      language: not-a-language
      pattern: "println!($$$ARGS)"
    - id: duplicate
      language: rust
      pattern: "println!($$$ARGS)"
    - id: duplicate
      language: rust
      pattern: "dbg!($$$ARGS)"
---

# Inline policy validation
"#,
    )
    .unwrap();

    criv(root)
        .args(["check", "--filter", "policy-pattern"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("missing-policy-pattern-id"))
        .stdout(predicate::str::contains("empty-policy-pattern"))
        .stdout(predicate::str::contains("missing-policy-pattern-language"))
        .stdout(predicate::str::contains("invalid-policy-pattern"))
        .stdout(predicate::str::contains(
            "missing-policy-pattern-definition",
        ))
        .stdout(predicate::str::contains("ambiguous-policy-pattern-body"))
        .stdout(predicate::str::contains("missing-policy-pattern-body"))
        .stdout(predicate::str::contains("duplicate-policy-pattern"))
        .stdout(predicate::str::contains(
            "unsupported ast-grep language `not-a-language`",
        ));
}

#[test]
fn adr_policy_patterns_are_the_only_persistent_named_searches() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs/adr")).unwrap();
    write_criv_config(
        root,
        vec!["src"],
        vec!["**/target/**", "**/node_modules/**"],
        true,
    );
    fs::write(
        root.join("src/governed.rs"),
        "pub fn governed() {\n    println!(\"governed\");\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/outside.rs"),
        "pub fn outside() {\n    println!(\"outside\");\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/adr/0990-pattern-search.md"),
        r#"---
id: ADR-0990
kind: decision
title: Pattern search
status: accepted
date: 2026-08-02
governs:
  - src/governed.rs
policy:
  patterns:
    - id: no-println
      language: rust
      pattern: "println!($$$ARGS)"
    - id: functions
      language: rust
      pattern: "pub fn $NAME() { $$$BODY }"
---

# Pattern search
"#,
    )
    .unwrap();

    criv(root)
        .args(["search", "--pattern-id", "ADR-0990/no-println"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/governed.rs:2"))
        .stdout(predicate::str::contains("src/outside.rs").not());
    criv(root)
        .args([
            "search",
            "--pattern-id",
            "ADR-0990/no-println",
            "--paths",
            "src/outside.rs",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/outside.rs:2"))
        .stdout(predicate::str::contains("src/governed.rs").not());
    criv(root)
        .args(["search", "--rule", "ADR-0990"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/governed.rs:1"))
        .stdout(predicate::str::contains("src/governed.rs:2"))
        .stdout(predicate::str::contains("src/outside.rs").not());
    criv(root)
        .args(["search", "--lang", "rust", "println!($$$ARGS)"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/governed.rs:2"))
        .stdout(predicate::str::contains("src/outside.rs:2"));
    criv(root)
        .args(["search", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--pattern-id <PATTERN_ID>"));
    criv(root)
        .arg("--usage")
        .assert()
        .success()
        .stdout(predicate::str::contains("flag --pattern-id"));

    fs::write(
        root.join("criv.toml"),
        r#"
[source]
roots = ["src"]

[patterns.no-println]
language = "rust"
pattern = "println!($$$ARGS)"
"#,
    )
    .unwrap();
    criv(root)
        .arg("check")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "[patterns.*] is no longer supported",
        ))
        .stderr(predicate::str::contains("ADR-NNNN/local-id"))
        .stderr(predicate::str::contains("--lang"));
}

#[test]
fn search_rule_reports_invalid_inline_policy_definitions() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("docs/adr")).unwrap();
    write_criv_config(
        root,
        vec!["src"],
        vec!["**/target/**", "**/node_modules/**"],
        false,
    );
    fs::write(
        root.join("docs/adr/0994-invalid-search-policy.md"),
        r#"---
id: ADR-0994
kind: decision
title: Invalid search policy
status: accepted
date: 2026-06-26
policy:
  patterns:
    - id: invalid-rule
      language: rust
      rule: "not: [valid"
---

# Invalid search policy
"#,
    )
    .unwrap();

    criv(root)
        .args(["search", "--rule", "ADR-0994"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("failed to parse ast-grep rule"));
}

#[test]
fn inline_policy_patterns_are_enforced_without_generation() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs/adr")).unwrap();
    write_criv_config(
        root,
        vec!["src"],
        vec!["**/target/**", "**/node_modules/**"],
        true,
    );
    fs::write(
        root.join("src/lib.rs"),
        "pub fn run() {\n    println!(\"blocked\");\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/adr/0995-inline-policy.md"),
        r#"---
id: ADR-0995
kind: decision
title: Inline policy
status: accepted
date: 2026-06-26
governs:
  - src/lib.rs#fn:run
policy:
  patterns:
    - id: no-println
      language: rust
      pattern: "println!($$$ARGS)"
---

# Inline policy
"#,
    )
    .unwrap();

    criv(root)
        .args(["check", "--filter", "policy-violation"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("ADR-0995 policy"))
        .stdout(predicate::str::contains("println!"));
    criv(root)
        .args(["search", "--rule", "ADR-0995"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/lib.rs:2"));
    criv(root)
        .args(["enforce", "--stage", "commit"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("ADR-0995 policy"));
    criv(root).args(["watch", "--once"]).assert().success();
    let state = fs::read_to_string(root.join(".criv/state.json")).unwrap();
    assert!(state.contains("ADR-0995/no-println"));
    assert!(state.contains("src/lib.rs"));
}

#[test]
fn check_and_ci_enforcement_scan_the_same_policy_ids_and_paths() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs/adr")).unwrap();
    write_criv_config(
        root,
        vec!["src"],
        vec!["**/target/**", "**/node_modules/**"],
        true,
    );
    fs::write(root.join("src/left.rs"), "fn left() {}\nstruct Left;\n").unwrap();
    fs::write(root.join("src/right.rs"), "fn right() {}\nstruct Right;\n").unwrap();
    fs::write(
        root.join("docs/adr/0991-policy-parity.md"),
        r#"---
id: ADR-0991
kind: decision
title: Policy parity
status: accepted
date: 2026-08-03
governs:
  - src/**
policy:
  patterns:
    - id: functions
      language: rust
      pattern: "fn $NAME() { $$$ }"
    - id: structs
      language: rust
      pattern: "struct $NAME;"
---

# Policy parity
"#,
    )
    .unwrap();

    let check = criv(root)
        .args(["check", "--filter", "policy-violation"])
        .output()
        .unwrap();
    let enforce = criv(root)
        .args(["enforce", "--stage", "ci"])
        .output()
        .unwrap();
    assert!(!check.status.success());
    assert!(!enforce.status.success());
    let check_stdout = String::from_utf8(check.stdout).unwrap();
    let enforce_stdout = String::from_utf8(enforce.stdout).unwrap();

    for expected in [
        "src/left.rs:1: ADR-0991 policy `ADR-0991/functions`",
        "src/right.rs:1: ADR-0991 policy `ADR-0991/functions`",
        "src/left.rs:2: ADR-0991 policy `ADR-0991/structs`",
        "src/right.rs:2: ADR-0991 policy `ADR-0991/structs`",
    ] {
        assert!(
            check_stdout.contains(expected),
            "missing from check: {expected}"
        );
        assert!(
            enforce_stdout.contains(expected),
            "missing from enforce: {expected}"
        );
    }
}

#[test]
fn non_accepted_inline_policies_remain_searchable_but_do_not_block_or_register_state() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs/adr")).unwrap();
    write_criv_config(
        root,
        vec!["src"],
        vec!["**/target/**", "**/node_modules/**"],
        true,
    );
    fs::write(
        root.join("src/lib.rs"),
        "pub fn run() {\n    println!(\"proposed\");\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/adr/0994-draft-policy.md"),
        r#"---
id: ADR-0994
kind: decision
title: Proposed inline policy
status: draft
date: 2026-08-02
governs:
  - src/lib.rs
policy:
  patterns:
    - id: no-println
      language: rust
      pattern: "println!($$$ARGS)"
---

# Proposed inline policy
"#,
    )
    .unwrap();

    criv(root)
        .args(["check", "--filter", "policy-violation"])
        .assert()
        .success();
    criv(root)
        .args(["enforce", "--stage", "ci"])
        .assert()
        .success();
    criv(root)
        .args(["search", "--pattern-id", "ADR-0994/no-println"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/lib.rs:2"));
    criv(root)
        .args(["search", "--rule", "ADR-0994"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/lib.rs:2"));
    criv(root).args(["watch", "--once"]).assert().success();
    let state = fs::read_to_string(root.join(".criv/state.json")).unwrap();
    assert!(!state.contains("ADR-0994/no-println"));
}

#[test]
fn commit_enforcement_scans_staged_governed_policy_files() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    git(root, &["init"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs/adr")).unwrap();
    write_criv_config(
        root,
        vec!["src"],
        vec!["**/target/**", "**/node_modules/**"],
        true,
    );
    fs::write(root.join("src/clean.rs"), "pub fn clean() {}\n").unwrap();
    fs::write(
        root.join("src/violating.rs"),
        "pub fn bad() {\n    println!(\"blocked\");\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/adr/0993-staged-policy.md"),
        r#"---
id: ADR-0993
kind: decision
title: Staged policy
status: accepted
date: 2026-06-26
governs:
  - src/**
policy:
  patterns:
    - id: no-println
      language: rust
      pattern: "println!($$$ARGS)"
---

# Staged policy
"#,
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial"]);

    fs::write(
        root.join("src/clean.rs"),
        "pub fn clean() -> bool { true }\n",
    )
    .unwrap();
    git(root, &["add", "src/clean.rs"]);
    criv(root)
        .args(["enforce", "--stage", "commit"])
        .assert()
        .success();

    fs::write(
        root.join("src/clean.rs"),
        "pub fn clean() {\n    println!(\"blocked too\");\n}\n",
    )
    .unwrap();
    git(root, &["add", "src/clean.rs"]);
    criv(root)
        .args(["enforce", "--stage", "commit"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("ADR-0993 policy"))
        .stdout(predicate::str::contains("src/clean.rs"));
}

#[test]
fn commit_enforcement_uses_the_requested_root_despite_inherited_git_context() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    git(root, &["init"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    fs::write(root.join("tracked.txt"), "before\n").unwrap();
    git(root, &["add", "tracked.txt"]);
    git(root, &["commit", "-m", "initial"]);
    fs::write(root.join("tracked.txt"), "after\n").unwrap();
    git(root, &["add", "tracked.txt"]);

    let outer = TempDir::new().unwrap();
    git(outer.path(), &["init"]);

    criv(root)
        .env("GIT_DIR", outer.path().join(".git"))
        .env("GIT_WORK_TREE", outer.path())
        .args(["enforce", "--stage", "commit"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1 staged files"));
}

#[test]
fn commit_enforcement_handles_an_unborn_head_without_a_git_executable() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    git(root, &["init"]);
    fs::write(root.join("tracked.txt"), "staged before the first commit\n").unwrap();
    git(root, &["add", "tracked.txt"]);

    let empty_path = TempDir::new().unwrap();
    criv(root)
        .env("PATH", empty_path.path())
        .args(["enforce", "--stage", "commit"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1 staged files"));
}

#[test]
fn manual_push_enforcement_runs_without_a_git_executable() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    git(root, &["init"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    fs::write(root.join("tracked.txt"), "before\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial"]);
    fs::write(root.join("tracked.txt"), "after\n").unwrap();
    git(root, &["add", "tracked.txt"]);
    git(root, &["commit", "-m", "change"]);

    let empty_path = TempDir::new().unwrap();
    criv(root)
        .env("PATH", empty_path.path())
        .args(["enforce", "--stage", "push"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1 changed files"));
}

#[test]
fn commit_enforcement_respects_selector_governed_policy_files() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    git(root, &["init"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs/adr")).unwrap();
    write_criv_config(
        root,
        vec!["src"],
        vec!["**/target/**", "**/node_modules/**"],
        true,
    );
    fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();
    fs::write(
        root.join("docs/adr/0992-selector-staged-policy.md"),
        r#"---
id: ADR-0992
kind: decision
title: Selector staged policy
status: accepted
date: 2026-06-26
governs:
  - src/lib.rs#fn:run
policy:
  patterns:
    - id: no-println
      language: rust
      pattern: "println!($$$ARGS)"
---

# Selector staged policy
"#,
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial"]);

    fs::write(
        root.join("src/lib.rs"),
        "pub fn run() {\n    println!(\"blocked\");\n}\n",
    )
    .unwrap();
    git(root, &["add", "src/lib.rs"]);

    criv(root)
        .args(["enforce", "--stage", "commit"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("ADR-0992 policy"))
        .stdout(predicate::str::contains("src/lib.rs"));
}

#[test]
fn import_policy_denies_grouped_and_aliased_rust_imports() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "use crate::{infra::db};\nuse crate::infra as infra;\npub fn run() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("criv.toml"),
        r#"[vault]
docs = "docs"
adr = "adr"

[source]
roots = ["src"]
exclude = ["**/target/**", "**/node_modules/**"]

[index]
source = true
embeddings = false

[enforce]
stages = ["commit", "push", "ci"]

[[enforce.imports]]
id = "no-infra"
scope = ["src/**"]
deny = ["crate::infra"]
"#,
    )
    .unwrap();

    criv(root)
        .args(["enforce", "--stage", "ci"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("import policy `no-infra`"))
        .stdout(predicate::str::contains("crate::infra"));
}

#[test]
fn watch_port_is_rejected() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);

    criv(root)
        .args(["watch", "--port", "1234"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--port"));
}

#[test]
fn long_running_watch_takes_lock_before_startup_rebuild() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();
    let state_path = root.join(".criv/state.json");
    fs::write(&state_path, "sentinel\n").unwrap();
    // A lock owned by a live process (this test) must be honored, so the
    // watcher fails before it can overwrite the sentinel state.
    fs::write(root.join(".criv/watch.lock"), live_pid_lock()).unwrap();

    criv(root)
        .arg("watch")
        .assert()
        .failure()
        .stderr(predicate::str::contains("watch.lock"));
    assert_eq!(fs::read_to_string(state_path).unwrap(), "sentinel\n");
}

#[test]
fn long_running_watch_rebuilds_docs_sources_bursts_and_recovers() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn initial() {}\n").unwrap();
    let config = fs::read_to_string(root.join("criv.toml")).unwrap();

    let mut watch = WatchProcess::spawn(root);
    watch.wait_until_running();

    fs::write(
        root.join("docs/live.md"),
        "---\nid: live\nkind: doc\ntitle: Live note\n---\n\n# Live note\n",
    )
    .unwrap();
    wait_for_state(root, "the docs update", |state| {
        state["graph"]["nodes"].as_array().is_some_and(|nodes| {
            nodes
                .iter()
                .any(|node| node["id"].as_str() == Some("note:live"))
        })
    });

    fs::write(root.join("src/lib.rs"), "pub fn source_update() {}\n").unwrap();
    wait_for_state(root, "the source update", |state| {
        state["graph"]["nodes"].as_array().is_some_and(|nodes| {
            nodes
                .iter()
                .any(|node| node["id"].as_str() == Some("symbol:src/lib.rs#fn:source_update"))
        })
    });

    let last_good_state = fs::read_to_string(root.join(".criv/state.json")).unwrap();
    fs::write(root.join("criv.toml"), "[source\nroots = [\"src\"]\n").unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn invalid_config_change() {}\n",
    )
    .unwrap();
    assert_state_remains(root, &last_good_state, "a failed rebuild");

    fs::write(root.join("criv.toml"), config).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn recovered() {}\n").unwrap();
    wait_for_state(root, "recovery after a failed rebuild", |state| {
        state["graph"]["nodes"].as_array().is_some_and(|nodes| {
            nodes
                .iter()
                .any(|node| node["id"].as_str() == Some("symbol:src/lib.rs#fn:recovered"))
        })
    });

    fs::write(root.join("src/lib.rs"), "pub fn burst_one() {}\n").unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn burst_two() {}\n").unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn burst_final() {}\n").unwrap();
    wait_for_state(root, "the final debounced source burst", |state| {
        state["graph"]["nodes"].as_array().is_some_and(|nodes| {
            nodes
                .iter()
                .any(|node| node["id"].as_str() == Some("symbol:src/lib.rs#fn:burst_final"))
        })
    });

    fs::rename(root.join("src/lib.rs"), root.join("src/renamed.rs")).unwrap();
    wait_for_state(root, "a renamed source file", |state| {
        state["graph"]["nodes"].as_array().is_some_and(|nodes| {
            nodes
                .iter()
                .any(|node| node["id"].as_str() == Some("code:src/renamed.rs"))
                && !nodes
                    .iter()
                    .any(|node| node["id"].as_str() == Some("code:src/lib.rs"))
        })
    });

    fs::remove_file(root.join("src/renamed.rs")).unwrap();
    wait_for_state(root, "a deleted source file", |state| {
        state["graph"]["nodes"].as_array().is_some_and(|nodes| {
            !nodes
                .iter()
                .any(|node| node["id"].as_str() == Some("code:src/renamed.rs"))
        })
    });
}

struct WatchProcess {
    child: Child,
    lines: Receiver<String>,
    stdout_reader: Option<JoinHandle<()>>,
}

impl WatchProcess {
    fn spawn(root: &Path) -> Self {
        let mut child = ProcessCommand::new(assert_cmd::cargo::cargo_bin("criv"))
            .current_dir(root)
            .env_remove("CI")
            .env_remove("GITHUB_ACTIONS")
            .env_remove("CRIV_BASE_REF")
            .env_remove("GITHUB_BASE_REF")
            .arg("watch")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start criv watch");
        let stdout = child.stdout.take().expect("watch stdout");
        let (tx, lines) = mpsc::channel();
        let stdout_reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if tx
                    .send(line.unwrap_or_else(|error| format!("<stdout error: {error}>")))
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            child,
            lines,
            stdout_reader: Some(stdout_reader),
        }
    }

    fn wait_until_running(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(45);
        let mut output = Vec::new();
        while Instant::now() < deadline {
            match self.lines.recv_timeout(Duration::from_millis(100)) {
                Ok(line) => {
                    if line == "criv watch running" {
                        return;
                    }
                    output.push(line);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if let Some(status) = self.child.try_wait().expect("poll watch process") {
                panic!("criv watch exited before startup with {status}; stdout: {output:?}");
            }
        }
        panic!("timed out waiting for criv watch running; stdout: {output:?}");
    }
}

impl Drop for WatchProcess {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(stdout_reader) = self.stdout_reader.take() {
            let _ = stdout_reader.join();
        }
    }
}

fn wait_for_state(root: &Path, description: &str, predicate: impl Fn(&serde_json::Value) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let state_path = root.join(".criv/state.json");
    let mut latest = String::new();
    while Instant::now() < deadline {
        if let Ok(contents) = fs::read_to_string(&state_path) {
            latest = contents;
            if let Ok(state) = serde_json::from_str(&latest)
                && predicate(&state)
            {
                return;
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {description}; last state: {latest}");
}

fn assert_state_remains(root: &Path, expected: &str, description: &str) {
    let deadline = Instant::now() + Duration::from_millis(800);
    while Instant::now() < deadline {
        let actual = fs::read_to_string(root.join(".criv/state.json")).unwrap();
        assert_eq!(actual, expected, "state changed during {description}");
        thread::sleep(Duration::from_millis(50));
    }
}

fn write_criv_config(root: &Path, roots: Vec<&str>, exclude: Vec<&str>, source_index: bool) {
    let config = toml::toml! {
        [vault]
        docs = "docs"
        adr = "adr"

        [source]
        roots = roots
        exclude = exclude

        [index]
        source = source_index
        embeddings = false

        [enforce]
        stages = ["commit", "push", "ci"]
    };
    fs::write(root.join("criv.toml"), config.to_string()).unwrap();
}

fn query_fixture(root: &Path) {
    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs/adr")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn run() {\n    helper();\n}\n\nfn helper() {}\n",
    )
    .unwrap();
    write_criv_config(
        root,
        vec!["src"],
        vec!["**/target/**", "**/node_modules/**"],
        true,
    );
    fs::write(
        root.join("docs/adr/0001-test-decision.md"),
        r#"---
id: ADR-0001
kind: decision
title: Test decision
status: accepted
governs:
  - src/**/*.rs
---

# Test decision
"#,
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

This cites [[ADR-0001]] and mentions [[src/lib.rs#fn:helper]].
"#,
    )
    .unwrap();
    fs::write(
        root.join("docs/orphan.md"),
        r#"---
id: orphan
kind: doc
title: Orphan
---

# Orphan
"#,
    )
    .unwrap();

    criv(root).args(["watch", "--once"]).assert().success();
}

#[test]
fn adr_reconcile_renumbers_a_branch_local_collision_and_rewrites_references() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    git(root, &["init", "-b", "target"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    git(root, &["config", "core.autocrlf", "true"]);
    init(root);
    write_criv_config(root, vec!["src"], vec![], true);
    fs::write(
        root.join("docs/adr/0001-base.md"),
        adr("0001", "Base", "base"),
    )
    .unwrap();
    fs::write(
        root.join("docs/guide.md"),
        "---\nid: guide\nkind: doc\ntitle: Guide\n---\n\n## Guide\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    let shared_boilerplate = "// this long boilerplate line is shared but has no ADR reference\n";
    fs::write(
        root.join("src/base.rs"),
        format!("// target-owned\n{shared_boilerplate}"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base"]);

    git(root, &["checkout", "-b", "topic"]);
    fs::write(
        root.join("docs/adr/0002-topic.md"),
        adr("0002", "Topic", "topic"),
    )
    .unwrap();
    git(root, &["mv", "docs/guide.md", "docs/guide-topic.md"]);
    fs::write(
        root.join("docs/guide-topic.md"),
        "---\nid: guide\nkind: doc\ntitle: Guide\n---\n\n## Guide\n\nSee [[0002-topic|ADR-0002]].\n",
    )
    .unwrap();
    fs::write(root.join("src/no-reference.rs"), shared_boilerplate).unwrap();
    fs::write(
        root.join("src/comment.rs"),
        "// target-owned\n// ADR-0002\npub fn topic() {}\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "topic adr"]);

    git(root, &["checkout", "target"]);
    fs::write(
        root.join("docs/adr/0002-target.md"),
        adr("0002", "Target", "target"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "target adr"]);
    git(root, &["checkout", "topic"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            root.join("src/comment.rs"),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();
    }

    let config = fs::read_to_string(root.join("criv.toml")).unwrap();
    fs::write(
        root.join("criv.toml"),
        format!("{config}\n# topic configuration change\n"),
    )
    .unwrap();
    criv(root)
        .args(["adr", "reconcile", "--base", "topic", "--check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("allocation is current"));
    criv(root)
        .env("CRIV_BASE_REF", "topic")
        .args(["enforce", "--stage", "ci"])
        .assert()
        .success();
    fs::write(
        root.join("criv.toml"),
        config.replace("docs = \"docs\"", "docs = \"other-docs\""),
    )
    .unwrap();
    criv(root)
        .args(["adr", "reconcile", "--base", "topic", "--check"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("vault.docs or vault.adr"));
    fs::write(root.join("criv.toml"), config).unwrap();

    criv(root)
        .args(["adr", "reconcile", "--base", "target", "--check"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("ADR-0002 -> ADR-0003"))
        .stderr(predicate::str::contains("criv adr reconcile --base target"));
    criv(root)
        .env("CRIV_BASE_REF", "target")
        .args(["enforce", "--stage", "ci"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("ADR-0002 -> ADR-0003"));
    git(root, &["config", "commit.gpgSign", "true"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let hook = root.join(".git/hooks/pre-commit");
        fs::write(
            &hook,
            "#!/bin/sh\ntouch \"$(git rev-parse --show-toplevel)/hook-ran\"\nexit 1\n",
        )
        .unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }
    criv(root)
        .args(["adr", "reconcile", "--base", "target"])
        .assert()
        .success();

    assert!(!root.join("docs/adr/0002-topic.md").exists());
    let topic_adr = fs::read_to_string(root.join("docs/adr/0003-topic.md")).unwrap();
    assert!(topic_adr.contains("id: ADR-0003"));
    assert!(
        fs::read_to_string(root.join("docs/guide-topic.md"))
            .unwrap()
            .contains("[[0003-topic|ADR-0003]]")
    );
    assert!(
        normalize_newlines(&fs::read_to_string(root.join("src/comment.rs")).unwrap())
            .contains("// target-owned\n// ADR-0003")
    );
    assert_eq!(
        normalize_newlines(&fs::read_to_string(root.join("src/no-reference.rs")).unwrap()),
        shared_boilerplate
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(root.join("src/comment.rs"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
    }
    assert!(root.join(".criv/adr-reconcile.json").exists());
    assert!(
        git_stdout(root, &["status", "--porcelain"])
            .trim()
            .is_empty()
    );
    assert_eq!(
        git_stdout(root, &["show", "-s", "--format=%s", "HEAD"]).trim(),
        "docs(adr): reconcile provisional identifiers"
    );
    assert_eq!(
        git_stdout(
            root,
            &["show", "-s", "--format=%an <%ae>|%cn <%ce>", "HEAD"]
        )
        .trim(),
        "criv <criv@example.com>|criv <criv@example.com>"
    );
    assert!(!git_stdout(root, &["cat-file", "-p", "HEAD"]).contains("gpgsig "));
    assert!(!root.join("hook-ran").exists());
    let reconciliation_commit = git_stdout(root, &["rev-parse", "HEAD"]).trim().to_owned();
    let reconciliation_parent = git_stdout(root, &["rev-parse", "HEAD^"]).trim().to_owned();

    criv(root)
        .args(["enforce", "--stage", "push"])
        .assert()
        .success();
    criv(root)
        .args([
            "enforce",
            "--stage",
            "push",
            "--pre-push",
            "--remote-name",
            "origin",
            "--remote-url",
            "https://example.invalid/criv.git",
        ])
        .write_stdin(format!(
            "refs/heads/topic {reconciliation_commit} refs/heads/topic {reconciliation_parent}\n"
        ))
        .assert()
        .success();

    let receipt_path = root.join(".criv/adr-reconcile.json");
    let receipt = fs::read_to_string(&receipt_path).unwrap();
    fs::write(
        &receipt_path,
        receipt.replace("criv.adr-reconcile/3", "forged-receipt"),
    )
    .unwrap();
    criv(root)
        .args([
            "enforce",
            "--stage",
            "push",
            "--pre-push",
            "--remote-name",
            "origin",
            "--remote-url",
            "https://example.invalid/criv.git",
        ])
        .write_stdin(format!(
            "refs/heads/topic {reconciliation_commit} refs/heads/topic {reconciliation_parent}\n"
        ))
        .assert()
        .failure()
        .stdout(predicate::str::contains("ADR files are immutable"));
    fs::write(&receipt_path, receipt).unwrap();
    criv(root)
        .env("CRIV_BASE_REF", "target")
        .args(["enforce", "--stage", "ci"])
        .assert()
        .success();
    git(root, &["config", "commit.gpgSign", "false"]);
    #[cfg(unix)]
    fs::remove_file(root.join(".git/hooks/pre-commit")).unwrap();
    git(root, &["merge", "target", "--no-edit"]);
    criv(root)
        .env("CRIV_BASE_REF", "target")
        .args(["enforce", "--stage", "ci"])
        .assert()
        .success();
    criv(root)
        .args(["adr", "reconcile", "--base", "target", "--check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("allocation is current"));
    criv(root)
        .args(["adr", "reconcile", "--base", "topic", "--check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("allocation is current"));
    criv(root)
        .args(["adr", "reconcile", "--base", "missing-target", "--check"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot resolve base ref"));
    fs::write(root.join("unrelated.txt"), "unrelated\n").unwrap();
    criv(root)
        .args(["adr", "reconcile", "--base", "target", "--check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("allocation is current"));
    fs::remove_file(root.join("unrelated.txt")).unwrap();
    git(root, &["checkout", "target"]);
    fs::write(
        root.join("docs/adr/0004-target-late.md"),
        adr("0004", "Target late", "target-late"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "advance target"]);
    git(root, &["checkout", "topic"]);
    criv(root)
        .args([
            "enforce",
            "--stage",
            "push",
            "--pre-push",
            "--remote-name",
            "origin",
            "--remote-url",
            "https://example.invalid/criv.git",
        ])
        .write_stdin(format!(
            "refs/heads/topic {reconciliation_commit} refs/heads/topic {reconciliation_parent}\n"
        ))
        .assert()
        .failure()
        .stdout(predicate::str::contains("ADR files are immutable"));
}

#[test]
fn adr_reconcile_renames_a_same_path_branch_local_adr() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    git(root, &["init", "-b", "target"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    init(root);
    write_criv_config(root, vec!["src"], vec![], true);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn base() {}\n").unwrap();
    fs::write(
        root.join("docs/adr/0001-base.md"),
        adr("0001", "Base", "base"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base"]);

    git(root, &["checkout", "-b", "topic"]);
    fs::write(
        root.join("docs/adr/0002-shared.md"),
        adr("0002", "Topic", "topic"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "topic adr"]);

    git(root, &["checkout", "target"]);
    fs::write(
        root.join("docs/adr/0002-shared.md"),
        adr("0002", "Target", "target"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "target adr"]);
    git(root, &["checkout", "topic"]);

    criv(root)
        .args(["adr", "reconcile", "--base", "target", "--check"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("ADR-0002 -> ADR-0003"));
    criv(root)
        .args(["adr", "reconcile", "--base", "target"])
        .assert()
        .success();
    assert!(!root.join("docs/adr/0002-shared.md").exists());
    assert!(
        fs::read_to_string(root.join("docs/adr/0003-shared.md"))
            .unwrap()
            .contains("title: Topic")
    );
}

#[test]
fn adr_reconcile_recognizes_and_retries_a_materialized_worktree() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    git(root, &["init", "-b", "target"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    init(root);
    write_criv_config(root, vec!["src"], vec![], true);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn base() {}\n").unwrap();
    fs::write(
        root.join("docs/adr/0001-base.md"),
        adr("0001", "Base", "base"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base"]);

    git(root, &["checkout", "-b", "topic"]);
    fs::write(
        root.join("docs/adr/0002-topic.md"),
        adr("0002", "Topic", "topic"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            root.join("docs/adr/0002-topic.md"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "topic adr"]);

    git(root, &["checkout", "target"]);
    fs::write(
        root.join("docs/adr/0002-target.md"),
        adr("0002", "Target", "target"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "target adr"]);
    git(root, &["checkout", "topic"]);

    criv(root)
        .args(["adr", "reconcile", "--base", "target"])
        .assert()
        .success();
    let reconciliation_parent = git_stdout(root, &["rev-parse", "HEAD^"]).trim().to_owned();
    assert!(
        git_stdout(root, &["status", "--porcelain"])
            .trim()
            .is_empty()
    );
    // Recreate a materialized, uncommitted transaction as older criv versions
    // could leave behind so retry behavior remains covered.
    git(root, &["reset", &reconciliation_parent]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let destination = root.join("docs/adr/0003-topic.md");
        assert_eq!(
            fs::metadata(&destination).unwrap().permissions().mode() & 0o777,
            0o755
        );
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o644)).unwrap();
        criv(root)
            .args(["adr", "reconcile", "--base", "target", "--check"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("does not match the materialized"));
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o755)).unwrap();
    }
    criv(root)
        .args(["adr", "reconcile", "--base", "target", "--check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("allocation is current"));
    git(root, &["add", "-A"]);
    criv(root)
        .args(["enforce", "--stage", "commit"])
        .assert()
        .success();
    git(root, &["reset", "HEAD", "--", "docs/adr/0003-topic.md"]);
    criv(root)
        .args(["enforce", "--stage", "commit"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("ADR files are immutable"));
    git(root, &["add", "docs/adr/0003-topic.md"]);
    let receipt_path = root.join(".criv/adr-reconcile.json");
    let receipt = fs::read_to_string(&receipt_path).unwrap();
    fs::write(
        &receipt_path,
        receipt.replace("criv.adr-reconcile/3", "forged-receipt"),
    )
    .unwrap();
    criv(root)
        .args(["enforce", "--stage", "commit"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("ADR files are immutable"));
    fs::write(&receipt_path, receipt).unwrap();
    criv(root)
        .args(["adr", "reconcile", "--base", "target", "--check"])
        .assert()
        .success();

    git(root, &["stash", "push", "-u"]);
    git(root, &["checkout", "target"]);
    fs::write(
        root.join("docs/adr/0003-target.md"),
        adr("0003", "Target next", "target-next"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "target next adr"]);
    git(root, &["checkout", "topic"]);
    git(root, &["stash", "pop"]);

    fs::write(root.join("unrelated.txt"), "unrelated\n").unwrap();
    criv(root)
        .args(["adr", "reconcile", "--base", "target"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not cover every dirty"));
    fs::remove_file(root.join("unrelated.txt")).unwrap();
    criv(root)
        .args(["adr", "reconcile", "--base", "target", "--check"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("ADR-0003 -> ADR-0004"));
    criv(root)
        .args(["adr", "reconcile", "--base", "target"])
        .assert()
        .success();
    assert!(!root.join("docs/adr/0002-topic.md").exists());
    assert!(!root.join("docs/adr/0003-topic.md").exists());
    assert!(root.join("docs/adr/0004-topic.md").exists());
    criv(root)
        .args(["adr", "reconcile", "--base", "target", "--check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("allocation is current"));
}

#[cfg(unix)]
#[test]
fn adr_reconcile_normalizes_git_modes_and_snapshots_overlapping_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().unwrap();
    let root = temp.path();
    git(root, &["init", "-b", "target"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    init(root);
    write_criv_config(root, vec!["src"], vec![], true);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn base() {}\n").unwrap();
    fs::write(
        root.join("docs/adr/0001-base.md"),
        adr("0001", "Base", "base"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base"]);

    git(root, &["checkout", "-b", "topic"]);
    for (id, title) in [
        ("0002", "Two"),
        ("0003", "Three"),
        ("0004", "Four"),
        ("0005", "Five"),
    ] {
        fs::write(
            root.join(format!("docs/adr/{id}-topic.md")),
            adr(id, title, "topic"),
        )
        .unwrap();
    }
    fs::set_permissions(
        root.join("docs/adr/0005-topic.md"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "topic adrs"]);

    git(root, &["checkout", "target"]);
    fs::write(
        root.join("docs/adr/0002-target.md"),
        adr("0002", "Target", "target"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "target adr"]);
    git(root, &["checkout", "topic"]);

    for (path, mode) in [
        ("docs/adr/0002-topic.md", 0o444),
        ("docs/adr/0003-topic.md", 0o640),
        ("docs/adr/0004-topic.md", 0o644),
        ("docs/adr/0005-topic.md", 0o755),
    ] {
        fs::set_permissions(root.join(path), fs::Permissions::from_mode(mode)).unwrap();
    }

    criv(root)
        .args(["adr", "reconcile", "--base", "target"])
        .assert()
        .success();
    let reconciliation_commit = git_stdout(root, &["rev-parse", "HEAD"]).trim().to_owned();
    let reconciliation_parent = git_stdout(root, &["rev-parse", "HEAD^"]).trim().to_owned();
    for (path, expected) in [
        ("docs/adr/0003-topic.md", 0o444),
        ("docs/adr/0004-topic.md", 0o640),
        ("docs/adr/0005-topic.md", 0o644),
        ("docs/adr/0006-topic.md", 0o755),
    ] {
        assert_eq!(
            fs::metadata(root.join(path)).unwrap().permissions().mode() & 0o777,
            expected,
            "permissions for {path}"
        );
    }

    let receipt_path = root.join(".criv/adr-reconcile.json");
    let receipt = fs::read_to_string(&receipt_path).unwrap();
    let receipt_json: serde_json::Value = serde_json::from_str(&receipt).unwrap();
    assert_eq!(receipt_json["schema"], "criv.adr-reconcile/3");
    let modes = receipt_json["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|file| {
            Some((
                file["path"].as_str()?.to_owned(),
                file["after_mode"].as_str()?.to_owned(),
            ))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(modes["docs/adr/0003-topic.md"], "100644");
    assert_eq!(modes["docs/adr/0004-topic.md"], "100644");
    assert_eq!(modes["docs/adr/0005-topic.md"], "100644");
    assert_eq!(modes["docs/adr/0006-topic.md"], "100755");

    fs::write(
        &receipt_path,
        receipt.replace("criv.adr-reconcile/3", "criv.adr-reconcile/2"),
    )
    .unwrap();
    criv(root)
        .args([
            "enforce",
            "--stage",
            "push",
            "--pre-push",
            "--remote-name",
            "origin",
            "--remote-url",
            "https://example.invalid/criv.git",
        ])
        .write_stdin(format!(
            "refs/heads/topic {reconciliation_commit} refs/heads/topic {reconciliation_parent}\n"
        ))
        .assert()
        .failure()
        .stdout(predicate::str::contains("ADR files are immutable"));
    fs::write(&receipt_path, receipt).unwrap();

    git(
        root,
        &["update-index", "--chmod=+x", "docs/adr/0003-topic.md"],
    );
    criv(root)
        .args(["enforce", "--stage", "commit"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("ADR files are immutable"));
    git(
        root,
        &["update-index", "--chmod=-x", "docs/adr/0003-topic.md"],
    );
    criv(root)
        .args(["enforce", "--stage", "commit"])
        .assert()
        .success();
    criv(root)
        .args(["enforce", "--stage", "push"])
        .assert()
        .success();
}

#[test]
fn adr_reconcile_rejects_a_renamed_published_adr() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    git(root, &["init", "-b", "target"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    init(root);
    write_criv_config(root, vec!["src"], vec![], true);
    fs::write(
        root.join("docs/adr/0001-base.md"),
        adr("0001", "Base", "base"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base"]);

    git(root, &["checkout", "-b", "topic"]);
    fs::write(
        root.join("docs/adr/0002-topic.md"),
        fs::read_to_string(root.join("docs/adr/0001-base.md"))
            .unwrap()
            .replace("ADR-0001", "ADR-0002"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "rename published adr"]);

    git(root, &["checkout", "target"]);
    fs::write(
        root.join("docs/adr/0002-target.md"),
        adr("0002", "Target", "target"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "target adr"]);
    git(root, &["checkout", "topic"]);

    criv(root)
        .args(["adr", "reconcile", "--base", "target", "--check"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "appears to carry published content",
        ));
}

#[test]
fn adr_reconcile_rejects_a_short_reference_carried_by_a_low_similarity_move() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    git(root, &["init", "-b", "target"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    git(root, &["config", "core.autocrlf", "true"]);
    init(root);
    write_criv_config(root, vec!["src"], vec![], true);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("docs/adr/0001-base.md"),
        adr("0001", "Base", "base"),
    )
    .unwrap();
    let inherited = format!(
        "ADR-0002\n{}",
        (0..100)
            .map(|index| format!("old inherited line {index}\n"))
            .collect::<String>()
    );
    fs::write(root.join("src/original.rs"), inherited).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base"]);

    git(root, &["checkout", "-b", "topic"]);
    fs::write(
        root.join("docs/adr/0002-topic.md"),
        adr("0002", "Topic", "topic"),
    )
    .unwrap();
    git(root, &["mv", "src/original.rs", "src/moved.rs"]);
    let moved = format!(
        "ADR-0002\n{}",
        (0..100)
            .map(|index| format!("new branch line {index}\n"))
            .collect::<String>()
    );
    fs::write(root.join("src/moved.rs"), &moved).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "topic"]);

    git(root, &["checkout", "target"]);
    fs::write(
        root.join("docs/adr/0002-target.md"),
        adr("0002", "Target", "target"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "target"]);
    git(root, &["checkout", "topic"]);

    criv(root)
        .args(["adr", "reconcile", "--base", "target"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "refusing to rewrite target-owned reference",
        ));
    assert_eq!(
        normalize_newlines(&fs::read_to_string(root.join("src/moved.rs")).unwrap()),
        moved
    );
    assert!(root.join("docs/adr/0002-topic.md").exists());
    assert!(!root.join("docs/adr/0003-topic.md").exists());
}

#[test]
fn adr_reconcile_proves_an_overlapping_mapping_transaction() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    git(root, &["init", "-b", "target"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    git(root, &["config", "core.autocrlf", "true"]);
    init(root);
    write_criv_config(root, vec!["src"], vec![], true);
    fs::write(
        root.join("docs/adr/0001-base.md"),
        adr("0001", "Base", "base"),
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn base() {}\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base"]);

    git(root, &["checkout", "-b", "topic"]);
    fs::write(
        root.join("docs/adr/0005-first.md"),
        adr("0005", "First", "first"),
    )
    .unwrap();
    fs::write(
        root.join("docs/adr/0007-second.md"),
        adr("0007", "Second", "second"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "topic adrs"]);

    git(root, &["checkout", "target"]);
    fs::write(
        root.join("docs/adr/0006-target.md"),
        adr("0006", "Target", "target"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "target adr"]);
    git(root, &["checkout", "topic"]);

    criv(root)
        .args(["adr", "reconcile", "--base", "target"])
        .assert()
        .success();
    assert!(root.join("docs/adr/0007-first.md").exists());
    assert!(root.join("docs/adr/0008-second.md").exists());
    git(root, &["add", "-A"]);
    criv(root)
        .args(["enforce", "--stage", "commit"])
        .assert()
        .success();
}

#[test]
fn adr_reconcile_requires_identity_before_mutating_the_transaction() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    adr_collision_fixture(root);
    git(root, &["config", "user.name", ""]);
    git(root, &["config", "user.email", ""]);
    let head = git_stdout(root, &["rev-parse", "HEAD"]);
    let source = fs::read_to_string(root.join("docs/adr/0002-topic.md")).unwrap();

    criv(root)
        .args(["adr", "reconcile", "--base", "target"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Git commit identity is unavailable",
        ));

    assert_eq!(git_stdout(root, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        fs::read_to_string(root.join("docs/adr/0002-topic.md")).unwrap(),
        source
    );
    assert!(!root.join("docs/adr/0003-topic.md").exists());
    assert!(!root.join(".criv/adr-reconcile.json").exists());
    assert!(
        git_stdout(root, &["status", "--porcelain"])
            .trim()
            .is_empty()
    );
}

#[test]
fn adr_reconcile_rolls_back_when_vault_validation_fails() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    adr_collision_fixture(root);
    fs::write(
        root.join("docs/broken.md"),
        "---\nid: broken\nkind: doc\ntitle: Broken\n---\n\nMissing [[missing-target]]\n",
    )
    .unwrap();
    git(root, &["add", "docs/broken.md"]);
    git(
        root,
        &["commit", "-m", "docs: add broken reference fixture"],
    );
    let head = git_stdout(root, &["rev-parse", "HEAD"]);
    let source = fs::read_to_string(root.join("docs/adr/0002-topic.md")).unwrap();

    criv(root)
        .args(["adr", "reconcile", "--base", "target"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("vault validation failed"));

    assert_eq!(git_stdout(root, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        fs::read_to_string(root.join("docs/adr/0002-topic.md")).unwrap(),
        source
    );
    assert!(!root.join("docs/adr/0003-topic.md").exists());
    assert!(!root.join(".criv/adr-reconcile.json").exists());
    assert!(
        git_stdout(root, &["status", "--porcelain"])
            .trim()
            .is_empty()
    );
}

#[test]
fn adr_reconcile_receipt_rejects_a_later_mutation_in_the_push_range() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    adr_collision_fixture(root);
    criv(root)
        .args(["adr", "reconcile", "--base", "target"])
        .assert()
        .success();
    let reconciliation_parent = git_stdout(root, &["rev-parse", "HEAD^"]).trim().to_owned();
    let reconciled = root.join("docs/adr/0003-topic.md");
    let mut contents = fs::read_to_string(&reconciled).unwrap();
    contents.push_str("\nLater mutation.\n");
    fs::write(&reconciled, contents).unwrap();
    git(root, &["add", "docs/adr/0003-topic.md"]);
    git(root, &["commit", "-m", "docs: mutate reconciled adr"]);
    let mutated_commit = git_stdout(root, &["rev-parse", "HEAD"]).trim().to_owned();

    criv(root)
        .args([
            "enforce",
            "--stage",
            "push",
            "--pre-push",
            "--remote-name",
            "origin",
            "--remote-url",
            "https://example.invalid/criv.git",
        ])
        .write_stdin(format!(
            "refs/heads/topic {mutated_commit} refs/heads/topic {reconciliation_parent}\n"
        ))
        .assert()
        .failure()
        .stdout(predicate::str::contains("ADR files are immutable"));
}

#[test]
fn adr_reconcile_uses_the_requested_root_despite_inherited_git_context() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    git(root, &["init", "-b", "target"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    init(root);
    fs::write(
        root.join("docs/adr/0001-base.md"),
        adr("0001", "Base", "base"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base"]);

    git(root, &["checkout", "-b", "topic"]);
    fs::write(
        root.join("docs/adr/0002-topic.md"),
        adr("0002", "Topic", "topic"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "topic adr"]);

    git(root, &["checkout", "target"]);
    fs::write(
        root.join("docs/adr/0002-target.md"),
        adr("0002", "Target", "target"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "target adr"]);
    git(root, &["checkout", "topic"]);

    let outer = TempDir::new().unwrap();
    git(outer.path(), &["init"]);
    let outer_git = outer.path().join(".git");
    criv(root)
        .env("GIT_DIR", &outer_git)
        .env("GIT_WORK_TREE", outer.path())
        .env("GIT_INDEX_FILE", outer_git.join("index"))
        .env("GIT_COMMON_DIR", &outer_git)
        .env("GIT_PREFIX", "outer")
        .args(["adr", "reconcile", "--base", "target", "--check"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("ADR-0002 -> ADR-0003"));
}

#[test]
fn adr_reconcile_detects_a_collision_from_a_linked_worktree() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    git(root, &["init", "-b", "target"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    init(root);
    fs::write(
        root.join("docs/adr/0001-base.md"),
        adr("0001", "Base", "base"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base"]);

    git(root, &["checkout", "-b", "topic"]);
    fs::write(
        root.join("docs/adr/0002-topic.md"),
        adr("0002", "Topic", "topic"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "topic adr"]);

    git(root, &["checkout", "target"]);
    fs::write(
        root.join("docs/adr/0002-target.md"),
        adr("0002", "Target", "target"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "target adr"]);

    let linked = root.join("linked-worktree");
    git(
        root,
        &[
            "worktree",
            "add",
            "--detach",
            linked.to_str().unwrap(),
            "topic",
        ],
    );
    criv(&linked)
        .args(["adr", "reconcile", "--base", "target", "--check"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("ADR-0002 -> ADR-0003"));
}

fn adr(id: &str, title: &str, slug: &str) -> String {
    format!(
        "---\nid: ADR-{id}\nkind: decision\ntitle: {title}\nstatus: accepted\ndate: 2026-08-02\n---\n\n## {title}\n\n{slug}\n"
    )
}

fn adr_collision_fixture(root: &Path) {
    git(root, &["init", "-b", "target"]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
    init(root);
    write_criv_config(root, vec!["src"], vec![], true);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn base() {}\n").unwrap();
    fs::write(
        root.join("docs/adr/0001-base.md"),
        adr("0001", "Base", "base"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base"]);

    git(root, &["checkout", "-b", "topic"]);
    fs::write(
        root.join("docs/adr/0002-topic.md"),
        adr("0002", "Topic", "topic"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "topic adr"]);

    git(root, &["checkout", "target"]);
    fs::write(
        root.join("docs/adr/0002-target.md"),
        adr("0002", "Target", "target"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "target adr"]);
    git(root, &["checkout", "topic"]);
}

fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_PREFIX")
        .args(args)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_stdout(root: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_PREFIX")
        .args(args)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
