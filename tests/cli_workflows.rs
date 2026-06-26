use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn criv(root: &Path) -> Command {
    let mut command = Command::cargo_bin("criv").expect("criv binary");
    command.current_dir(root);
    command.env_remove("CI");
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

[patterns."ADR-0997/no-println"]
language = "rust"
pattern = "println!($$$ARGS)"
"#,
    )
    .unwrap();
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
