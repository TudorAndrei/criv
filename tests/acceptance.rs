// Cucumber requires &mut World and parses captures into owned FromStr values.
#![allow(
    clippy::expect_used,
    clippy::needless_pass_by_ref_mut,
    clippy::needless_pass_by_value,
    clippy::panic
)]

mod support;

use std::process::Output;

use cucumber::{World as _, given, then, when};
use tempfile::TempDir;

#[derive(Debug, cucumber::World)]
#[world(init = Self::new)]
struct VaultWorld {
    directory: TempDir,
    output: Option<Output>,
}

impl VaultWorld {
    fn new() -> Self {
        Self {
            directory: TempDir::new().expect("temporary scenario directory"),
            output: None,
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

#[when(
    regex = r"^I run criv (check|query next-adr-id|watch --once|enforce --stage ci|check --changed)$"
)]
fn run_command(world: &mut VaultWorld, arguments: String) {
    world.output = Some(
        support::criv(world.directory.path())
            .args(arguments.split_whitespace())
            .output()
            .expect("run criv"),
    );
}

#[when(regex = r"^I check staged changes in (text|json|github) format$")]
fn check_staged(world: &mut VaultWorld, format: String) {
    world.output = Some(
        support::criv(world.directory.path())
            .args(["check", "--changed", "--format", &format])
            .output()
            .expect("run staged check"),
    );
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

fn main() {
    futures::executor::block_on(
        VaultWorld::cucumber()
            .fail_on_skipped()
            .run_and_exit(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/features")),
    );
}
