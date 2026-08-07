#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn criv(root: &Path) -> Command {
    let mut command = Command::cargo_bin("criv").expect("criv binary");
    command.current_dir(root);
    command
}

fn write_editor(bin: &Path, name: &str, body: &str) {
    let path = bin.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn write_vsix(root: &Path) -> std::path::PathBuf {
    let path = root.join("viewer.vsix");
    fs::write(&path, "local viewer package").unwrap();
    path
}

#[test]
fn installs_the_local_vsix_with_the_selected_editor() {
    let temp = TempDir::new().unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let args = temp.path().join("args");
    write_editor(
        &bin,
        "cursor",
        "printf '%s\\n' \"$@\" > \"$CRIV_EDITOR_ARGS\"",
    );
    write_editor(&bin, "code", "exit 91");
    let vsix = write_vsix(temp.path());

    criv(temp.path())
        .args(["install-editor", "--editor", "cursor", "--vsix"])
        .arg(&vsix)
        .env("PATH", &bin)
        .env("CRIV_EDITOR_ARGS", &args)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "installed the criv viewer in cursor",
        ));

    let recorded = fs::read_to_string(args).unwrap();
    assert_eq!(
        recorded,
        format!(
            "--install-extension\n{}\n",
            fs::canonicalize(vsix).unwrap().display()
        )
    );
}

#[test]
fn dry_run_validates_without_starting_the_editor() {
    let temp = TempDir::new().unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let marker = temp.path().join("started");
    write_editor(&bin, "code", "touch \"$CRIV_EDITOR_MARKER\"");
    let vsix = write_vsix(temp.path());

    criv(temp.path())
        .args(["install-editor", "--editor", "code", "--vsix"])
        .arg(&vsix)
        .arg("--dry-run")
        .env("PATH", &bin)
        .env("CRIV_EDITOR_MARKER", &marker)
        .assert()
        .success()
        .stdout(predicate::str::contains("editor install dry run:"));

    assert!(!marker.exists());
}

#[test]
fn rejects_a_bad_vsix_before_starting_the_editor() {
    let temp = TempDir::new().unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let marker = temp.path().join("started");
    write_editor(&bin, "code", "touch \"$CRIV_EDITOR_MARKER\"");

    criv(temp.path())
        .args([
            "install-editor",
            "--editor",
            "code",
            "--vsix",
            "missing.vsix",
        ])
        .env("PATH", &bin)
        .env("CRIV_EDITOR_MARKER", &marker)
        .assert()
        .failure()
        .stderr(predicate::str::contains("VSIX path is not a readable file"));

    assert!(!marker.exists());
}

#[test]
fn missing_editor_names_the_selected_cli_and_the_alternative() {
    let temp = TempDir::new().unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let marker = temp.path().join("cursor-started");
    write_editor(&bin, "cursor", "touch \"$CRIV_EDITOR_MARKER\"");
    let vsix = write_vsix(temp.path());

    criv(temp.path())
        .args(["install-editor", "--editor", "code", "--vsix"])
        .arg(&vsix)
        .env("PATH", &bin)
        .env("CRIV_EDITOR_MARKER", &marker)
        .assert()
        .failure()
        .stderr(predicate::str::contains("code was not found on PATH"))
        .stderr(predicate::str::contains("--editor cursor"));

    assert!(!marker.exists());
}

#[test]
fn editor_failure_preserves_stdout_and_stderr() {
    let temp = TempDir::new().unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    write_editor(
        &bin,
        "code",
        "printf 'install output\\n'; printf 'install error\\n' >&2; exit 17",
    );
    let vsix = write_vsix(temp.path());

    criv(temp.path())
        .args(["install-editor", "--editor", "code", "--vsix"])
        .arg(&vsix)
        .env("PATH", &bin)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "code --install-extension exited 17",
        ))
        .stderr(predicate::str::contains("install output"))
        .stderr(predicate::str::contains("install error"));
}
