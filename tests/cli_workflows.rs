use std::fs;
use std::path::Path;

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
        .args(["init", "--no-hooks", "--no-obsidian", "--no-skills"])
        .assert()
        .success();
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
fn watch_once_does_not_rebuild_while_watch_lock_is_held() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    init(root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").unwrap();

    criv(root).args(["watch", "--once"]).assert().success();
    let state_before = fs::read_to_string(root.join(".criv/state.json")).unwrap();
    fs::write(root.join(".criv/watch.lock"), "active").unwrap();
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
    fs::write(root.join(".criv/watch.lock"), "held\n").unwrap();

    criv(root)
        .arg("watch")
        .assert()
        .failure()
        .stderr(predicate::str::contains("watch.lock"));
    assert_eq!(fs::read_to_string(state_path).unwrap(), "sentinel\n");
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
