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

#[given("no Git repository")]
fn no_git_repository(world: &mut VaultWorld) {
    assert!(!world.directory.path().join(".git").exists());
}

#[given("a Git repository with no staged changes")]
fn git_repository(world: &mut VaultWorld) {
    support::git_repository(world.directory.path(), "main");
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
