use std::collections::BTreeMap;
use std::path::PathBuf;

use super::*;

#[test]
fn init_writes_parseable_structured_templates() {
    let root = unique_temp_dir("criv-init-templates");
    run(
        &root,
        InitOptions {
            no_skills: true,
            force_skills: false,
        },
    )
    .unwrap();

    let config = std::fs::read_to_string(root.join("criv.toml")).unwrap();
    assert_eq!(config, include_str!("fixtures/criv.toml"));
    toml::from_str::<toml::Value>(&config).unwrap();
    assert!(!config.contains("languages"));
    assert!(!config.contains("notes ="));
    assert!(!config.contains("[obsidian]"));
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

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_does_not_create_editor_files() {
    let root = unique_temp_dir("criv-init-no-editor-files");

    run(&root, fast_options()).unwrap();

    assert!(!root.join(".obsidian").exists());
    assert!(!root.join(".vscode/extensions.json").exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_installs_c4_authoring_skill() {
    let root = unique_temp_dir("criv-init-c4-authoring-skill");
    let options = InitOptions {
        no_skills: false,
        force_skills: false,
    };

    run(&root, options).unwrap();

    for path in [
        ".agents/skills/c4-authoring/SKILL.md",
        ".claude/skills/c4-authoring/SKILL.md",
    ] {
        let skill = std::fs::read_to_string(root.join(path)).unwrap();
        assert!(skill.contains("name: c4-authoring"));
        assert!(skill.contains("`docs/architecture/` is one LikeC4 workspace"));
        assert!(skill.contains("LikeC4 merges every source file in the workspace into one model"));
        assert!(skill.contains("The work is complete when the vault is green"));
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_force_skills_refreshes_existing_skills_but_plain_init_preserves_them() {
    let root = unique_temp_dir("criv-init-force-skills");
    let mut options = fast_options();
    options.no_skills = false;
    run(&root, options).unwrap();

    let path = root.join(".agents/skills/criv/SKILL.md");
    std::fs::write(&path, "locally stale\n").unwrap();
    let mut plain = fast_options();
    plain.no_skills = false;
    run(&root, plain).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "locally stale\n");

    let mut forced = fast_options();
    forced.no_skills = false;
    forced.force_skills = true;
    run(&root, forced).unwrap();
    assert_eq!(
        crate::generated_skills::inventory(&root).status(".agents/skills/criv/SKILL.md"),
        Some(crate::generated_skills::InstalledSkillStatus::Current)
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_force_skills_respects_no_skills() {
    let root = unique_temp_dir("criv-init-force-no-skills");
    let mut options = fast_options();
    options.force_skills = true;
    run(&root, options).unwrap();
    assert!(!root.join(".agents/skills").exists());
    assert!(!root.join(".claude/skills").exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_force_skills_isolates_refresh_from_other_scaffolding() {
    let root = unique_temp_dir("criv-init-force-skills-isolated");
    let mut options = fast_options();
    options.force_skills = true;
    options.no_skills = false;
    run(&root, options).unwrap();

    assert!(root.join(".agents/skills/criv/SKILL.md").exists());
    assert!(root.join(".claude/skills/criv/SKILL.md").exists());
    for path in [
        "criv.toml",
        ".gitignore",
        ".obsidian",
        ".vscode/extensions.json",
    ] {
        assert!(!root.join(path).exists(), "force-skills created {path}");
    }

    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn init_force_skills_refuses_symlinked_destination() {
    let root = unique_temp_dir("criv-init-force-skills-symlink");
    let outside = unique_temp_dir("criv-init-force-skills-target");
    std::fs::create_dir_all(root.join(".agents/skills/criv")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join(".agents/skills/criv/SKILL.md")).unwrap();
    let mut options = fast_options();
    options.no_skills = false;
    options.force_skills = true;
    let error = run(&root, options).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("refusing to write through symlinked vault path component")
    );
    assert!(!outside.join("SKILL.md").exists());

    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}

fn fast_options() -> InitOptions {
    InitOptions {
        no_skills: true,
        force_skills: false,
    }
}

/// Per ADR-0044, documentation that a project governs has to be versioned in the
/// repository it governs. Scaffolding a vault through a symlinked `docs/` would
/// write the governance graph somewhere Git never sees, so it is refused.
#[cfg(unix)]
#[test]
fn init_refuses_to_scaffold_through_a_symlinked_docs_directory() {
    let root = unique_temp_dir("criv-init-symlink-docs");
    let outside = unique_temp_dir("criv-init-symlink-docs-target");
    std::os::unix::fs::symlink(&outside, root.join("docs")).unwrap();

    let error = run(
        &root,
        InitOptions {
            no_skills: true,
            force_skills: false,
        },
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("refusing to write through symlinked vault path component"),
        "unexpected error: {error}"
    );
    assert!(
        !outside.join("adr").exists(),
        "nothing may be created outside the repository"
    );

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&outside);
}

#[cfg(unix)]
#[test]
fn init_refuses_to_write_a_template_through_a_symlinked_state_directory() {
    let root = unique_temp_dir("criv-init-symlink-state");
    let outside = unique_temp_dir("criv-init-symlink-state-target");
    std::os::unix::fs::symlink(&outside, root.join(".criv")).unwrap();

    let error = run(
        &root,
        InitOptions {
            no_skills: true,
            force_skills: false,
        },
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("refusing to write through symlinked vault path component"),
        "unexpected error: {error}"
    );
    assert!(
        !outside.join("state.json").exists(),
        "nothing may be written outside the repository"
    );

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&outside);
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
