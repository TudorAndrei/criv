use std::collections::BTreeMap;
use std::path::PathBuf;

use super::*;

#[test]
fn init_writes_parseable_structured_templates() {
    let root = unique_temp_dir("criv-init-templates");
    run(
        &root,
        InitOptions {
            no_obsidian: false,
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
fn init_installs_git_hooks_by_default() {
    let root = unique_temp_dir("criv-init-hooks");
    git2::Repository::init(&root).unwrap();

    run(&root, fast_options()).unwrap();

    let repo = git2::Repository::open(&root).unwrap();
    assert_eq!(
        repo.config().unwrap().get_string("core.hooksPath").unwrap(),
        ".githooks"
    );

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
    assert!(pre_push.contains("\"$CRIV_BIN\" enforce --stage push"));

    assert_executable(root.join(".githooks/pre-commit"));
    assert_executable(root.join(".githooks/pre-push"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_hooks_cd_to_nested_criv_root() {
    let root = unique_temp_dir("criv-init-hooks-nested");
    git2::Repository::init(&root).unwrap();
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
    git2::Repository::init(&root).unwrap();

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
    let repo = git2::Repository::init(&root).unwrap();
    repo.config()
        .unwrap()
        .set_str("core.hooksPath", "custom-hooks")
        .unwrap();
    std::fs::create_dir_all(root.join(".githooks")).unwrap();
    std::fs::write(root.join(".githooks/pre-push"), "#!/bin/sh\necho custom\n").unwrap();

    let mut options = fast_options();
    options.force_hooks = true;
    run(&root, options).unwrap();

    assert_eq!(
        repo.config().unwrap().get_string("core.hooksPath").unwrap(),
        ".githooks"
    );
    assert!(
        std::fs::read_to_string(root.join(".githooks/pre-push"))
            .unwrap()
            .contains("\"$CRIV_BIN\" enforce --stage push")
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_preserves_existing_non_criv_hookspath_without_force() {
    let root = unique_temp_dir("criv-init-hooks-existing-hookspath");
    let repo = git2::Repository::init(&root).unwrap();
    repo.config()
        .unwrap()
        .set_str("core.hooksPath", "custom-hooks")
        .unwrap();

    run(&root, fast_options()).unwrap();

    assert_eq!(
        repo.config().unwrap().get_string("core.hooksPath").unwrap(),
        "custom-hooks"
    );
    assert!(root.join(".githooks/pre-commit").exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_no_hooks_skips_hook_installation() {
    let root = unique_temp_dir("criv-init-hooks-disabled");
    git2::Repository::init(&root).unwrap();
    let mut options = fast_options();
    options.no_hooks = true;

    run(&root, options).unwrap();

    assert!(!root.join(".githooks").exists());
    assert!(
        git2::Repository::open(&root)
            .unwrap()
            .config()
            .unwrap()
            .get_string("core.hooksPath")
            .is_err()
    );

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
    git2::Repository::init_bare(&root).unwrap();

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

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{unique}"));
    std::fs::create_dir_all(&path).unwrap();
    path
}
