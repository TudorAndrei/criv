use std::collections::BTreeMap;

use tempfile::TempDir;

use super::*;
use crate::config::Config;

fn fast_options() -> InitOptions {
    InitOptions {
        no_skills: true,
        force_skills: false,
    }
}

fn with_skills(force_skills: bool) -> InitOptions {
    InitOptions {
        no_skills: false,
        force_skills,
    }
}

const SYMLINK_REFUSAL: &str = "refusing to write through symlinked vault path component";

#[test]
fn init_writes_loadable_configuration_state_and_decision_index() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    run(root, &fast_options()).unwrap();

    let config = std::fs::read_to_string(root.join("criv.toml")).unwrap();
    Config::parse(Some(&config)).unwrap();
    serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(root.join(".criv/state.json")).unwrap(),
    )
    .unwrap();

    let readme = std::fs::read_to_string(root.join("docs/adr/README.md")).unwrap();
    let frontmatter = readme
        .strip_prefix("---\n")
        .and_then(|value| value.split_once("---\n"))
        .map(|(frontmatter, _body)| frontmatter)
        .unwrap();
    serde_norway::from_str::<BTreeMap<String, serde_norway::Value>>(frontmatter).unwrap();
}

#[test]
fn init_force_skills_refreshes_existing_skills_but_plain_init_preserves_them() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    run(root, &with_skills(false)).unwrap();

    let path = root.join(".agents/skills/criv/SKILL.md");
    std::fs::write(&path, "locally stale\n").unwrap();
    run(root, &with_skills(false)).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "locally stale\n");

    run(root, &with_skills(true)).unwrap();
    assert_ne!(std::fs::read_to_string(&path).unwrap(), "locally stale\n");
    assert_eq!(
        crate::install::outdated_skills_advisory(&RepositoryFiles::open(root).unwrap()),
        None
    );
}

#[test]
fn init_force_skills_isolates_refresh_from_other_scaffolding() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    run(root, &with_skills(true)).unwrap();

    assert!(root.join(".agents/skills/criv/SKILL.md").exists());
    assert!(root.join(".claude/skills/criv/SKILL.md").exists());
    for path in ["criv.toml", ".gitignore", "docs", ".criv"] {
        assert!(!root.join(path).exists(), "force-skills created {path}");
    }
}

#[cfg(unix)]
#[test]
fn init_force_skills_refuses_symlinked_destination() {
    let temp = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join(".agents/skills/criv")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.join(".agents/skills/criv/SKILL.md")).unwrap();
    let error = run(root, &with_skills(true)).unwrap_err();
    assert!(error.to_string().contains(SYMLINK_REFUSAL), "{error}");
    assert!(!outside.path().join("SKILL.md").exists());
}

/// Per ADR-0044, documentation that a project governs has to be versioned in the
/// repository it governs. Scaffolding a vault through a symlinked `docs/` would
/// write the governance graph somewhere Git never sees, so it is refused.
#[cfg(unix)]
#[test]
fn init_refuses_to_scaffold_through_a_symlinked_docs_directory() {
    let temp = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), temp.path().join("docs")).unwrap();

    let error = run(temp.path(), &fast_options()).unwrap_err();

    assert!(error.to_string().contains(SYMLINK_REFUSAL), "{error}");
    assert!(
        !outside.path().join("adr").exists(),
        "nothing may be created outside the repository"
    );
}

#[cfg(unix)]
#[test]
fn init_refuses_to_write_a_template_through_a_symlinked_state_directory() {
    let temp = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), temp.path().join(".criv")).unwrap();

    let error = run(temp.path(), &fast_options()).unwrap_err();

    assert!(error.to_string().contains(SYMLINK_REFUSAL), "{error}");
    assert!(
        !outside.path().join("state.json").exists(),
        "nothing may be written outside the repository"
    );
}
