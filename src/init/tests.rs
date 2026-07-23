use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

#[test]
fn init_writes_parseable_structured_templates() {
    let root = unique_temp_dir("criv-init-templates");
    run(
        &root,
        InitOptions {
            no_obsidian: false,
            no_vscode: false,
            no_skills: true,
            no_hooks: false,
            force_hooks: false,
        },
    )
    .unwrap();

    let config = std::fs::read_to_string(root.join("criv.toml")).unwrap();
    toml::from_str::<toml::Value>(&config).unwrap();
    assert!(!config.contains("languages"));
    assert!(!config.contains("notes ="));
    assert!(!config.contains("[obsidian]"));
    serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(root.join(".criv/state.json")).unwrap(),
    )
    .unwrap();

    for path in [
        ".obsidian/app.json",
        ".obsidian/plugins/criv/manifest.json",
        ".obsidian/plugins/criv/package.json",
        ".obsidian/plugins/criv/tsconfig.json",
        ".obsidian/plugins/criv/versions.json",
    ] {
        serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(root.join(path)).unwrap(),
        )
        .unwrap();
    }
    let obsidian_app: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join(".obsidian/app.json")).unwrap())
            .unwrap();
    let ignore_filters = obsidian_app["userIgnoreFilters"].as_array().unwrap();
    for ignored in [
        ".criv/",
        ".git/",
        "target/",
        ".obsidian/plugins/criv/node_modules/",
        ".obsidian/plugins/criv/pkg/",
    ] {
        assert!(
            ignore_filters
                .iter()
                .any(|value| value.as_str() == Some(ignored)),
            "missing Obsidian ignore filter {ignored}"
        );
    }

    let plugin_package: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join(".obsidian/plugins/criv/package.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(plugin_package["devDependencies"]["esbuild"], "0.28.1");
    assert_eq!(plugin_package["allowScripts"]["esbuild@0.28.1"], true);
    assert!(!root.join(".obsidian/plugins/criv/main.js").exists());

    let readme = std::fs::read_to_string(root.join("docs/adr/README.md")).unwrap();
    let frontmatter = readme
        .strip_prefix("---\n")
        .and_then(|value| value.split_once("---\n"))
        .map(|(frontmatter, _body)| frontmatter)
        .unwrap();
    serde_norway::from_str::<BTreeMap<String, serde_norway::Value>>(frontmatter).unwrap();

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_creates_vscode_extension_recommendation_by_default() {
    let root = unique_temp_dir("criv-init-vscode-recommendation");

    run(&root, fast_options()).unwrap();

    let recommendations = vscode_recommendations(&root);
    assert_eq!(recommendations, vec!["criv.vscode-criv"]);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_preserves_existing_vscode_extension_recommendations() {
    let root = unique_temp_dir("criv-init-vscode-preserve");
    std::fs::create_dir_all(root.join(".vscode")).unwrap();
    std::fs::write(
        root.join(".vscode/extensions.json"),
        r#"{
  "recommendations": ["rust-lang.rust-analyzer"],
  "unwantedRecommendations": ["example.unwanted"]
}
"#,
    )
    .unwrap();

    run(&root, fast_options()).unwrap();

    let value = vscode_extensions_json(&root);
    assert_eq!(
        value["recommendations"].as_array().unwrap(),
        &vec![
            serde_json::Value::String("rust-lang.rust-analyzer".to_string()),
            serde_json::Value::String("criv.vscode-criv".to_string()),
        ]
    );
    assert_eq!(
        value["unwantedRecommendations"].as_array().unwrap(),
        &vec![serde_json::Value::String("example.unwanted".to_string())]
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_avoids_duplicate_vscode_extension_recommendations() {
    let root = unique_temp_dir("criv-init-vscode-duplicates");
    std::fs::create_dir_all(root.join(".vscode")).unwrap();
    std::fs::write(
        root.join(".vscode/extensions.json"),
        r#"{"recommendations":["criv.vscode-criv"]}"#,
    )
    .unwrap();

    run(&root, fast_options()).unwrap();
    run(&root, fast_options()).unwrap();

    let recommendations = vscode_recommendations(&root);
    assert_eq!(recommendations, vec!["criv.vscode-criv"]);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_no_vscode_skips_extension_recommendation() {
    let root = unique_temp_dir("criv-init-vscode-disabled");
    let mut options = fast_options();
    options.no_vscode = true;

    run(&root, options).unwrap();

    assert!(!root.join(".vscode/extensions.json").exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_installs_git_hooks_by_default() {
    let root = unique_temp_dir("criv-init-hooks");
    git_init(&root);

    run(&root, fast_options()).unwrap();

    assert_eq!(git_config(&root, "core.hooksPath").unwrap(), ".githooks");

    let pre_commit = std::fs::read_to_string(root.join(".githooks/pre-commit")).unwrap();
    assert!(pre_commit.contains("cd '.'"));
    assert!(pre_commit.contains("CRIV_BIN=\"$(command -v criv)\""));
    assert!(pre_commit.contains("CRIV_BIN=\"./target/debug/criv\""));
    assert!(pre_commit.contains("\"$CRIV_BIN\" watch --once"));
    assert!(pre_commit.contains("\"$CRIV_BIN\" check"));
    assert!(pre_commit.contains("\"$CRIV_BIN\" enforce --stage commit"));

    let pre_push = std::fs::read_to_string(root.join(".githooks/pre-push")).unwrap();
    assert!(pre_push.contains("cd '.'"));
    assert!(pre_push.contains("CRIV_BIN=\"$(command -v criv)\""));
    assert!(pre_push.contains(
        "\"$CRIV_BIN\" enforce --stage push --pre-push --remote-name \"$1\" --remote-url \"$2\""
    ));

    assert_executable(root.join(".githooks/pre-commit"));
    assert_executable(root.join(".githooks/pre-push"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_hooks_cd_to_nested_criv_root() {
    let root = unique_temp_dir("criv-init-hooks-nested");
    git_init(&root);
    let vault = root.join("docs-vault");
    std::fs::create_dir_all(&vault).unwrap();

    run(&vault, fast_options()).unwrap();

    let pre_commit = std::fs::read_to_string(root.join(".githooks/pre-commit")).unwrap();
    assert!(pre_commit.contains("cd 'docs-vault'"));
    assert!(vault.join("criv.toml").exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_hooks_are_idempotent_without_force() {
    let root = unique_temp_dir("criv-init-hooks-idempotent");
    git_init(&root);

    run(&root, fast_options()).unwrap();
    let hook = root.join(".githooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\necho custom\n").unwrap();

    run(&root, fast_options()).unwrap();

    assert_eq!(
        std::fs::read_to_string(&hook).unwrap(),
        "#!/bin/sh\necho custom\n"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_force_hooks_overwrites_hooks_and_hookspath() {
    let root = unique_temp_dir("criv-init-hooks-force");
    git_init(&root);
    git_config_set(&root, "core.hooksPath", "custom-hooks");
    std::fs::create_dir_all(root.join(".githooks")).unwrap();
    std::fs::write(root.join(".githooks/pre-push"), "#!/bin/sh\necho custom\n").unwrap();

    let mut options = fast_options();
    options.force_hooks = true;
    run(&root, options).unwrap();

    assert_eq!(git_config(&root, "core.hooksPath").unwrap(), ".githooks");
    assert!(
        std::fs::read_to_string(root.join(".githooks/pre-push"))
            .unwrap()
            .contains("\"$CRIV_BIN\" enforce --stage push --pre-push")
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_preserves_existing_non_criv_hookspath_without_force() {
    let root = unique_temp_dir("criv-init-hooks-existing-hookspath");
    git_init(&root);
    git_config_set(&root, "core.hooksPath", "custom-hooks");

    run(&root, fast_options()).unwrap();

    assert_eq!(git_config(&root, "core.hooksPath").unwrap(), "custom-hooks");
    assert!(root.join(".githooks/pre-commit").exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_no_hooks_skips_hook_installation() {
    let root = unique_temp_dir("criv-init-hooks-disabled");
    git_init(&root);
    let mut options = fast_options();
    options.no_hooks = true;

    run(&root, options).unwrap();

    assert!(!root.join(".githooks").exists());
    assert!(git_config(&root, "core.hooksPath").is_none());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_outside_git_repo_skips_hooks_without_failing() {
    let root = unique_temp_dir("criv-init-hooks-no-git");

    run(&root, fast_options()).unwrap();

    assert!(root.join("criv.toml").exists());
    assert!(!root.join(".githooks").exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_bare_git_repo_skips_hooks_without_failing() {
    let root = unique_temp_dir("criv-init-hooks-bare");
    git_init_bare(&root);

    run(&root, fast_options()).unwrap();

    assert!(root.join("criv.toml").exists());
    assert!(!root.join(".githooks").exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_installs_c4_authoring_skill() {
    let root = unique_temp_dir("criv-init-c4-authoring-skill");
    let options = InitOptions {
        no_obsidian: true,
        no_vscode: true,
        no_skills: false,
        no_hooks: true,
        force_hooks: false,
    };

    run(&root, options).unwrap();

    for path in [
        ".agents/skills/c4-authoring/SKILL.md",
        ".claude/skills/c4-authoring/SKILL.md",
    ] {
        let skill = std::fs::read_to_string(root.join(path)).unwrap();
        assert!(skill.contains("name: c4-authoring"));
        assert!(skill.contains("Standalone `.c4` files are a filetype convention"));
        assert!(skill.contains("Prefer stable interface-bearing anchors"));
    }

    let _ = std::fs::remove_dir_all(root);
}

fn fast_options() -> InitOptions {
    InitOptions {
        no_obsidian: true,
        no_vscode: false,
        no_skills: true,
        no_hooks: false,
        force_hooks: false,
    }
}

#[cfg(unix)]
fn assert_executable(path: PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let mode = std::fs::metadata(path).unwrap().permissions().mode();
    assert_ne!(mode & 0o111, 0);
}

#[cfg(not(unix))]
fn assert_executable(_path: PathBuf) {}

fn vscode_recommendations(root: &std::path::Path) -> Vec<String> {
    vscode_extensions_json(root)["recommendations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_string())
        .collect()
}

fn vscode_extensions_json(root: &std::path::Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(root.join(".vscode/extensions.json")).unwrap())
        .unwrap()
}

fn git_init(root: &Path) {
    git(root, &["init"]);
}

fn git_init_bare(root: &Path) {
    git(root, &["init", "--bare"]);
}

fn git_config(root: &Path, key: &str) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["config", key])
        .output()
        .unwrap();
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_config_set(root: &Path, key: &str, value: &str) {
    git(root, &["config", key, value]);
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git -C {} {} failed: {}{}",
        root.display(),
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{unique}"));
    std::fs::create_dir_all(&path).unwrap();
    path
}
