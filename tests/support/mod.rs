#![allow(clippy::expect_used, dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use assert_cmd::Command;

/// Variables through which the test process's own Git repository, Git hook,
/// or CI run would leak into a fixture.
const INHERITED_CONTEXT: [&str; 11] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_PREFIX",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "CI",
    "GITHUB_ACTIONS",
    "CRIV_BASE_REF",
    "GITHUB_BASE_REF",
];

fn isolated(program: &Path, root: &Path) -> ProcessCommand {
    let mut command = ProcessCommand::new(program);
    command.current_dir(root);
    for variable in INHERITED_CONTEXT {
        command.env_remove(variable);
    }
    command
}

fn built_criv() -> PathBuf {
    assert_cmd::cargo::cargo_bin("criv")
}

/// The built criv binary, run in `root` without inherited repository context.
pub fn criv(root: &Path) -> Command {
    criv_at(&built_criv(), root)
}

/// A criv `executable` at another location, run like [`criv`].
pub fn criv_at(executable: &Path, root: &Path) -> Command {
    Command::from_std(isolated(executable, root))
}

/// The built criv binary as a process the test spawns and manages itself.
pub fn criv_process(root: &Path) -> ProcessCommand {
    isolated(&built_criv(), root)
}

/// A Git repository at `root` on `branch` with a commit identity, so fixture
/// commits and criv's own commits never depend on the host's Git setup.
pub fn git_repository(root: &Path, branch: &str) {
    git(root, &["init", "-b", branch]);
    git(root, &["config", "user.email", "criv@example.com"]);
    git(root, &["config", "user.name", "criv"]);
}

pub fn git(root: &Path, args: &[&str]) {
    git_stdout(root, args);
}

/// Runs the Git CLI in `root` with signing disabled and returns its standard
/// output; a failing command fails the test.
pub fn git_stdout(root: &Path, args: &[&str]) -> String {
    let output = isolated(Path::new("git"), root)
        .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
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
    String::from_utf8(output.stdout).expect("git output is UTF-8")
}
