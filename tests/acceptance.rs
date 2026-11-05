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

#[given("an initialized vault without a Git repository")]
fn initialized_vault(world: &mut VaultWorld) {
    support::criv(world.directory.path())
        .args(["init", "--no-skills"])
        .assert()
        .success();
}

#[given("a Git repository with no staged changes")]
fn git_repository(world: &mut VaultWorld) {
    // The CLI embeds Git; setup also avoids requiring a Git executable.
    git2::Repository::init(world.directory.path()).expect("initialize Git repository");
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
    let repository = git2::Repository::init(root).expect("initialize Git repository");
    let tree = stage_all(&repository);
    let signature = git2::Signature::now("criv", "criv@example.com").expect("signature");
    repository
        .commit(Some("HEAD"), &signature, &signature, "decide", &tree, &[])
        .expect("commit decision");
    world.decision = Some(decision);
}

#[given("the decision is edited and staged")]
fn edit_and_stage_decision(world: &mut VaultWorld) {
    let root = world.directory.path();
    let decision = root.join(world.decision.as_ref().expect("commit a decision first"));
    let mut contents = std::fs::read_to_string(&decision).expect("read decision");
    contents.push_str("\nA later change of mind.\n");
    std::fs::write(&decision, contents).expect("edit decision");
    stage_all(&git2::Repository::open(root).expect("open Git repository"));
}

fn stage_all(repository: &git2::Repository) -> git2::Tree<'_> {
    let mut index = repository.index().expect("Git index");
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .expect("stage files");
    index.write().expect("write Git index");
    let tree = index.write_tree().expect("write Git tree");
    repository.find_tree(tree).expect("find Git tree")
}

#[when(
    regex = r"^I run criv (check|query next-adr-id|watch --once|enforce --stage ci|enforce --stage commit|check --changed)$"
)]
fn run_command(world: &mut VaultWorld, arguments: String) {
    world.output = Some(
        support::criv(world.directory.path())
            .args(arguments.split_whitespace())
            .output()
            .expect("run criv"),
    );
}

#[when(regex = r"^I check staged changes in (default|text|json|github) format$")]
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
