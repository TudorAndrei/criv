// Cucumber requires &mut World and parses captures into owned FromStr values.
#![allow(
    clippy::expect_used,
    clippy::needless_pass_by_ref_mut,
    clippy::needless_pass_by_value,
    clippy::panic
)]

mod support;

use std::path::{Path, PathBuf};
use std::process::Output;

use cucumber::{World as _, cli, cli::Parser as _, given, then, when};
use tempfile::TempDir;

#[derive(Debug, cucumber::World)]
#[world(init = Self::new)]
struct VaultWorld {
    directory: TempDir,
    output: Option<Output>,
    decision: Option<PathBuf>,
}

impl VaultWorld {
    fn new() -> Self {
        Self {
            directory: TempDir::new().expect("temporary scenario directory"),
            output: None,
            decision: None,
        }
    }

    const fn output(&self) -> &Output {
        self.output.as_ref().expect("run a command first")
    }
}

#[given("an empty directory")]
const fn empty_directory(_world: &mut VaultWorld) {}

#[given("an initialized vault")]
fn initialized_vault(world: &mut VaultWorld) {
    support::criv(world.directory.path())
        .args(["init", "--no-skills"])
        .assert()
        .success();
}

/// Git discovery walks up from the vault, so no ancestor may hold a repository.
#[given("no Git repository")]
fn no_git_repository(world: &mut VaultWorld) {
    let enclosing = world
        .directory
        .path()
        .ancestors()
        .find(|directory| directory.join(".git").exists());
    assert!(
        enclosing.is_none(),
        "the scenario directory is inside the Git repository at {enclosing:?}"
    );
}

#[given("a Git repository with no staged changes")]
fn git_repository(world: &mut VaultWorld) {
    support::git_repository(world.directory.path(), "main");
}

#[given("a committed vault")]
fn committed_vault(world: &mut VaultWorld) {
    support::git_repository(world.directory.path(), "main");
    initialized_vault(world);
    commit_all(world.directory.path(), "initial vault");
}

#[given(expr = "a committed note {string} that links to {string}")]
fn committed_note_with_link(world: &mut VaultWorld, id: String, link: String) {
    write_note(world.directory.path(), &id, &format!("See [[{link}]]."));
    commit_all(world.directory.path(), "add note");
}

#[given(expr = "a committed note {string} without links")]
fn committed_note(world: &mut VaultWorld, id: String) {
    write_note(world.directory.path(), &id, "Valid content.");
    commit_all(world.directory.path(), "add note");
}

#[given(expr = "a staged note {string} that links to {string}")]
fn staged_note_with_link(world: &mut VaultWorld, id: String, link: String) {
    write_note(world.directory.path(), &id, &format!("See [[{link}]]."));
    support::git(world.directory.path(), &["add", "docs"]);
}

#[given(expr = "the note {string} is {word} and staged")]
fn staged_note_change(world: &mut VaultWorld, id: String, change: String) {
    let note = format!("docs/{id}.md");
    let root = world.directory.path();
    match change.as_str() {
        "renamed" => support::git(root, &["mv", &note, "docs/renamed-note.md"]),
        "deleted" => support::git(root, &["rm", &note]),
        other => panic!("unknown staged change {other}"),
    }
}

#[given("a committed decision that forbids println in Rust sources")]
fn committed_println_policy(world: &mut VaultWorld) {
    let root = world.directory.path();
    std::fs::create_dir_all(root.join("src")).expect("create source directory");
    std::fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").expect("write source");
    std::fs::write(
        root.join("docs/adr/0999-no-println.md"),
        "---\nid: ADR-0999\nkind: decision\ntitle: No println\nstatus: accepted\ndate: 2026-08-03\ngoverns:\n  - src/**/*.rs\npolicy:\n  patterns:\n    - id: no-println\n      language: rust\n      pattern: \"println!($$$ARGS)\"\n---\n\n# No println\n",
    )
    .expect("write decision");
    commit_all(root, "add source policy");
}

#[given("a staged Rust source that calls println")]
fn staged_println_source(world: &mut VaultWorld) {
    let root = world.directory.path();
    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn run() {\n    println!(\"blocked\");\n}\n",
    )
    .expect("write source");
    support::git(root, &["add", "src/lib.rs"]);
}

#[given("a Rust source")]
fn rust_source(world: &mut VaultWorld) {
    let root = world.directory.path();
    std::fs::create_dir_all(root.join("src")).expect("create source directory");
    std::fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").expect("write source");
}

#[given("a committed Rust source that calls println")]
fn committed_println_source(world: &mut VaultWorld) {
    let root = world.directory.path();
    std::fs::create_dir_all(root.join("src")).expect("create source directory");
    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn run() {\n    println!(\"blocked\");\n}\n",
    )
    .expect("write source");
    commit_all(root, "add source");
}

#[given("a staged decision that forbids println in Rust sources")]
fn staged_println_policy(world: &mut VaultWorld) {
    let root = world.directory.path();
    std::fs::write(
        root.join("docs/adr/20261117-no-println.md"),
        "---\nid: ADR-20261117-no-println\nkind: decision\ntitle: No println\nstatus: accepted\ndate: 2026-11-17\ngoverns:\n  - src/**/*.rs\npolicy:\n  patterns:\n    - id: no-println\n      language: rust\n      pattern: \"println!($$$ARGS)\"\n---\n\n# No println\n",
    )
    .expect("write decision");
    support::git(root, &["add", "docs"]);
}

#[given(expr = "a staged legacy decision {int} {string}")]
fn staged_legacy_decision(world: &mut VaultWorld, number: u32, title: String) {
    let root = world.directory.path();
    let (file, id) = legacy_decision(number, &title);
    write_decision(root, &file, &id, &title);
    support::git(root, &["add", "docs"]);
}

#[given("a topic branch that adds a note")]
fn topic_branch_note(world: &mut VaultWorld) {
    let root = world.directory.path();
    support::git(root, &["checkout", "-q", "-b", "topic"]);
    write_note(root, "topic-note", "Valid content.");
    commit_all(root, "topic note");
}

#[given(expr = "a topic branch that adds legacy decision {int} {string}")]
fn topic_branch_legacy_decision(world: &mut VaultWorld, number: u32, title: String) {
    let root = world.directory.path();
    support::git(root, &["checkout", "-q", "-b", "topic"]);
    let (file, id) = legacy_decision(number, &title);
    write_decision(root, &file, &id, &title);
    commit_all(root, "topic decision");
}

#[given(expr = "a topic branch that adds dated decision {string}")]
fn topic_branch_dated_decision(world: &mut VaultWorld, title: String) {
    let root = world.directory.path();
    support::git(root, &["checkout", "-q", "-b", "topic"]);
    let file = format!("20261117-{}", slug(&title));
    write_decision(root, &file, &format!("ADR-{file}"), &title);
    commit_all(root, "topic decision");
}

#[given(expr = "a decision {string} in the file {string} with id {string}")]
fn decision_file(world: &mut VaultWorld, title: String, file: String, id: String) {
    write_decision(world.directory.path(), &file, &id, &title);
}

fn slug(title: &str) -> String {
    title.to_lowercase().replace(' ', "-")
}

/// The file stem and id of a legacy `NNNN` decision.
fn legacy_decision(number: u32, title: &str) -> (String, String) {
    (
        format!("{number:04}-{}", slug(title)),
        format!("ADR-{number:04}"),
    )
}

/// A complete accepted decision at `docs/adr/<file>.md` and the source it
/// governs.
fn write_decision(root: &Path, file: &str, id: &str, title: &str) {
    std::fs::create_dir_all(root.join("src")).expect("create source directory");
    std::fs::write(root.join("src/lib.rs"), "pub fn run() {}\n").expect("write source");
    std::fs::write(
        root.join(format!("docs/adr/{file}.md")),
        format!(
            "---\nid: {id}\nkind: decision\ntitle: {title}\nstatus: accepted\ndate: 2026-11-06\ngoverns:\n  - src/lib.rs\n---\n\n# {title}\n\n## Context\n\n{title} needs a context.\n\n## Decision\n\nChoose {title}.\n\n## Consequences\n\n{title} follows.\n"
        ),
    )
    .expect("write decision");
}

fn write_note(root: &Path, id: &str, body: &str) {
    std::fs::write(
        root.join(format!("docs/{id}.md")),
        format!("---\nid: {id}\nkind: doc\ntitle: {id}\n---\n\n# {id}\n\n{body}\n"),
    )
    .expect("write note");
}

fn commit_all(root: &Path, message: &str) {
    support::git(root, &["add", "."]);
    support::git(root, &["commit", "-m", message]);
}

#[given(expr = "a committed vault with documentation at {string} and one decision")]
fn committed_vault_with_decision(world: &mut VaultWorld, docs: String) {
    let root = world.directory.path();
    std::fs::write(
        root.join("criv.toml"),
        format!("[vault]\ndocs = \"{docs}\"\n"),
    )
    .expect("write configuration");
    let decision = Path::new(&docs).join("adr/0001-choose-storage.md");
    std::fs::create_dir_all(root.join(&decision).parent().expect("decision directory"))
        .expect("create decision directory");
    std::fs::write(
        root.join(&decision),
        "---\nid: ADR-0001\nkind: decision\ntitle: Choose Storage\nstatus: accepted\n---\n\n# Choose Storage\n",
    )
    .expect("write decision");
    support::git_repository(root, "main");
    support::git(root, &["add", "."]);
    support::git(root, &["commit", "-m", "decide"]);
    world.decision = Some(decision);
}

#[given("the decision is edited and staged")]
fn edit_and_stage_decision(world: &mut VaultWorld) {
    let root = world.directory.path();
    let decision = root.join(world.decision.as_ref().expect("commit a decision first"));
    let mut contents = std::fs::read_to_string(&decision).expect("read decision");
    contents.push_str("\nA later change of mind.\n");
    std::fs::write(&decision, contents).expect("edit decision");
    support::git(root, &["add", "."]);
}

#[when(regex = r"^I run criv (.+)$")]
fn run_command(world: &mut VaultWorld, arguments: String) {
    world.output = Some(
        support::criv(world.directory.path())
            .args(arguments.split_whitespace())
            .output()
            .expect("run criv"),
    );
}

#[when(expr = "I enforce the CI stage in JSON against the main branch named by {word}")]
fn enforce_ci_against_main(world: &mut VaultWorld, variable: String) {
    world.output = Some(
        support::criv(world.directory.path())
            .env(variable, "main")
            .args(["enforce", "--stage", "ci", "--format", "json"])
            .output()
            .expect("run CI enforcement"),
    );
}

#[when(expr = "I create the decision {string} on {word}")]
fn create_decision(world: &mut VaultWorld, title: String, date: String) {
    world.output = Some(
        support::criv(world.directory.path())
            .args(["adr", "new", &title, "--date", &date])
            .output()
            .expect("run criv adr new"),
    );
}

#[when(expr = "I check staged changes in {word} format")]
fn check_staged(world: &mut VaultWorld, format: String) {
    let mut command = support::criv(world.directory.path());
    command.args(["check", "--changed"]);
    if format != "default" {
        command.args(["--format", &format]);
    }
    world.output = Some(command.output().expect("run staged check"));
}

#[then("the command succeeds")]
fn succeeds(world: &mut VaultWorld) {
    let output = world.output();
    assert!(output.status.success(), "{output:?}");
}

#[then("the command fails")]
fn fails(world: &mut VaultWorld) {
    let output = world.output();
    assert!(!output.status.success(), "{output:?}");
}

#[then(expr = "the command fails with exit code {int}")]
fn fails_with_code(world: &mut VaultWorld, code: i32) {
    let output = world.output();
    assert_eq!(output.status.code(), Some(code), "{output:?}");
}

#[then(expr = "stdout contains {string}")]
fn stdout_contains(world: &mut VaultWorld, expected: String) {
    let actual = String::from_utf8_lossy(&world.output().stdout);
    assert!(
        actual.contains(&expected),
        "expected {expected:?} in {actual:?}"
    );
}

#[then(expr = "stdout does not contain {string}")]
fn stdout_lacks(world: &mut VaultWorld, unexpected: String) {
    let actual = String::from_utf8_lossy(&world.output().stdout);
    assert!(
        !actual.contains(&unexpected),
        "unexpected {unexpected:?} in {actual:?}"
    );
}

#[then(expr = "stderr contains {string}")]
fn stderr_contains(world: &mut VaultWorld, expected: String) {
    let actual = String::from_utf8_lossy(&world.output().stderr);
    assert!(
        actual.contains(&expected),
        "expected {expected:?} in {actual:?}"
    );
}

#[then(expr = "stdout is {string}")]
fn stdout_is(world: &mut VaultWorld, expected: String) {
    assert_eq!(String::from_utf8_lossy(&world.output().stdout), expected);
}

#[then(expr = "stdout is the line {string}")]
fn stdout_is_line(world: &mut VaultWorld, expected: String) {
    assert_eq!(
        String::from_utf8_lossy(&world.output().stdout),
        format!("{expected}\n")
    );
}

/// The docstring is the whole file. The Gherkin parser keeps the newline
/// after the opening delimiter, which is not file content.
#[then(expr = "the file {string} is:")]
fn file_is(world: &mut VaultWorld, path: String, step: &cucumber::gherkin::Step) {
    let docstring = step
        .docstring
        .as_deref()
        .expect("give the file as a docstring");
    let expected = docstring.strip_prefix('\n').unwrap_or(docstring);
    let actual = std::fs::read_to_string(world.directory.path().join(path)).expect("read file");
    assert_eq!(actual, expected);
}

/// `expected` is a comma-separated list of the ADR file names other than the
/// index.
#[then(expr = "the ADR files are {string}")]
fn adr_files_are(world: &mut VaultWorld, expected: String) {
    let mut files = std::fs::read_dir(world.directory.path().join("docs/adr"))
        .expect("read the ADR directory")
        .map(|entry| entry.expect("read an ADR entry").file_name())
        .filter_map(|name| name.into_string().ok())
        .filter(|name| name != "README.md")
        .collect::<Vec<_>>();
    files.sort();
    let expected = expected
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(files, expected);
}

/// An empty `code` names a passing report.
#[then(expr = "stdout is one JSON report with code {string}")]
fn stdout_is_json_report(world: &mut VaultWorld, code: String) {
    let report: serde_json::Value =
        serde_json::from_slice(&world.output().stdout).expect("stdout is one JSON value");
    assert_eq!(
        report.get("ok").and_then(serde_json::Value::as_bool),
        Some(code.is_empty()),
        "{report}"
    );
    let expected = (!code.is_empty()).then_some(code.as_str());
    assert_eq!(
        report.get("code").and_then(serde_json::Value::as_str),
        expected,
        "{report}"
    );
}

#[then(expr = "the JSON report fix is {string}")]
fn json_report_fix(world: &mut VaultWorld, fix: String) {
    let report: serde_json::Value =
        serde_json::from_slice(&world.output().stdout).expect("stdout is one JSON value");
    assert_eq!(
        report.get("fix").and_then(serde_json::Value::as_str),
        Some(fix.as_str()),
        "{report}"
    );
}

/// The single test name this harness reports to cargo-nextest.
const SUITE_NAME: &str = "acceptance";

fn main() {
    let mut arguments: Vec<String> = std::env::args().collect();
    if arguments.iter().any(|argument| argument == "--list") {
        if !arguments.iter().any(|argument| argument == "--ignored") {
            println!("{SUITE_NAME}: test");
        }
        return;
    }
    if arguments.iter().any(|argument| argument == "--exact") {
        arguments.retain(|argument| {
            !matches!(argument.as_str(), SUITE_NAME | "--exact" | "--nocapture")
        });
    }
    let options = cli::Opts::<_, _, _, cli::Empty>::parse_from(arguments);
    futures::executor::block_on(
        VaultWorld::cucumber()
            .fail_on_skipped()
            .with_cli(options)
            .run_and_exit(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/features")),
    );
}
