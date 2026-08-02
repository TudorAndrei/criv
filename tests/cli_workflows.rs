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
    command.env_remove("CI");
    command.env_remove("GITHUB_ACTIONS");
    command.env_remove("CRIV_BASE_REF");
    command.env_remove("GITHUB_BASE_REF");
    command
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
fn query_usage_errors_are_reported() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    query_fixture(root);

    criv(root)
        .args(["query", "callers"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "query `callers` requires <symbol>",
        ));
    criv(root)
        .args(["query", "bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("query `bogus` is not implemented"));
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
        .stderr(predicate::str::contains("requires <ref-a> <ref-b>"));
    criv(root)
        .args(["query", "diff", "nonexistent", "latest"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not resolve"));
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
    - id: no-language
      pattern: "println!($$$ARGS)"
    - id: invalid-rule
      language: rust
      rule: "not: [valid"
    - id: id-only
---

# Inline policy validation
"#,
    )
    .unwrap();

    criv(root)
        .args(["check", "--filter", "policy-pattern"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("missing-policy-pattern-language"))
        .stdout(predicate::str::contains("invalid-policy-pattern"))
        .stdout(predicate::str::contains(
            "missing-policy-pattern-definition",
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
