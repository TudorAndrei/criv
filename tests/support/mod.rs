#![allow(clippy::expect_used)]

use std::path::Path;

use assert_cmd::Command;

pub fn criv(root: &Path) -> Command {
    let mut command = Command::cargo_bin("criv").expect("criv binary");
    command.current_dir(root);
    // Fixtures own their repository context, including when run from Git hooks.
    for variable in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_PREFIX",
        "CI",
        "GITHUB_ACTIONS",
        "CRIV_BASE_REF",
        "GITHUB_BASE_REF",
    ] {
        command.env_remove(variable);
    }
    command
}
