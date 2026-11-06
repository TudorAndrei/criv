//! Lifecycle for the generated runtime skills shipped with criv.
//!
//! Callers install the canonical inventory or ask for the best-effort
//! outdated-skills advisory.
//! Template identity, marker handling, confined publication, and the
//! `.claude/skills` link-or-copy policy stay inside this module.

use std::path::Path;

use crate::markdown;
use crate::repository::{LinkLayout, LinkOutcome, RepositoryFiles, RepositoryWriteScope};
use crate::{CrivError, Result};

const AGENT_SKILLS_DIR: &str = ".agents/skills";
const CLAUDE_SKILLS_DIR: &str = ".claude/skills";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InstallMode {
    CreateOnly,
    Refresh,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SkillPublication {
    Created,
    Refreshed,
    Preserved,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SkillPublicationFact {
    pub(crate) path: String,
    pub(crate) publication: SkillPublication,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ClaudePublication {
    Current,
    Linked,
    Replaced,
    Copied,
    Blocked,
}

#[derive(Debug, Eq, PartialEq)]
pub struct InstallReport {
    skills: Vec<SkillPublicationFact>,
    claude: ClaudePublication,
}

impl InstallReport {
    pub(crate) fn skill_publications(&self) -> &[SkillPublicationFact] {
        &self.skills
    }

    /// What happened to `.claude/skills`, for the user.
    pub(crate) const fn claude_message(&self) -> &'static str {
        match self.claude {
            ClaudePublication::Current => {
                "skipped .claude/skills: already linked to .agents/skills"
            }
            ClaudePublication::Linked => "linked .claude/skills to .agents/skills",
            ClaudePublication::Replaced => {
                "replaced copied .claude/skills with a link to .agents/skills"
            }
            ClaudePublication::Copied => {
                "copied .claude/skills: this platform does not support directory links"
            }
            ClaudePublication::Blocked => {
                "skipped .claude/skills: holds copied files; run `criv init --force-skills` to replace them with a link"
            }
        }
    }
}

/// The text note that tells a user to refresh outdated agent skills.
pub fn outdated_skills_advisory(files: &RepositoryFiles) -> Option<String> {
    let outdated = outdated_paths(files).len();
    let subject = match outdated {
        0 => return None,
        1 => "skill is",
        _ => "skills are",
    };
    Some(format!(
        "note: {outdated} agent {subject} out of date; run `criv init --force-skills`"
    ))
}

/// The installed skills a refresh would change: a copied `.claude/skills`
/// and every skill whose marker is stale or absent. A missing or unreadable
/// skill is not reported; the advisory is best-effort.
fn outdated_paths(files: &RepositoryFiles) -> Vec<String> {
    let copied = matches!(
        files.link_layout(Path::new(CLAUDE_SKILLS_DIR), Path::new(AGENT_SKILLS_DIR)),
        Ok(LinkLayout::Directory)
    );
    copied
        .then(|| CLAUDE_SKILLS_DIR.to_owned())
        .into_iter()
        .chain(
            SKILLS
                .iter()
                .filter(|skill| is_outdated(files, skill))
                .map(SkillTemplate::agent_path),
        )
        .collect()
}

pub fn install_skills(files: &RepositoryFiles, mode: InstallMode) -> Result<InstallReport> {
    files
        .link_layout(Path::new(CLAUDE_SKILLS_DIR), Path::new(AGENT_SKILLS_DIR))
        .map_err(|error| {
            CrivError::new(format!(
                "cannot install Claude skills because `.claude` must be a real repository directory: {error}"
            ))
        })?;
    let scope = files.write_scope(Path::new("."))?;
    let skills = SKILLS
        .iter()
        .map(|skill| publish_skill(files, &scope, skill.agent_path(), skill.contents, mode))
        .collect::<Result<Vec<_>>>()?;

    let link_outcome = scope.link_dir(
        Path::new(CLAUDE_SKILLS_DIR),
        Path::new(AGENT_SKILLS_DIR),
        mode == InstallMode::Refresh,
    )?;
    let claude = match link_outcome {
        LinkOutcome::Unchanged => ClaudePublication::Current,
        LinkOutcome::Created => ClaudePublication::Linked,
        LinkOutcome::Replaced => ClaudePublication::Replaced,
        LinkOutcome::DirectoryInTheWay => ClaudePublication::Blocked,
        LinkOutcome::Unsupported => {
            copy_claude_skills(files, &scope, mode)?;
            ClaudePublication::Copied
        }
    };

    Ok(InstallReport { skills, claude })
}

/// The `.claude/skills` fallback for a platform without directory links.
fn copy_claude_skills(
    files: &RepositoryFiles,
    scope: &RepositoryWriteScope<'_>,
    mode: InstallMode,
) -> Result<()> {
    for skill in SKILLS {
        publish_skill(files, scope, skill.claude_path(), skill.contents, mode)?;
    }
    Ok(())
}

fn publish_skill(
    files: &RepositoryFiles,
    scope: &RepositoryWriteScope<'_>,
    path: String,
    contents: &str,
    mode: InstallMode,
) -> Result<SkillPublicationFact> {
    let contents = stamp_skill(contents);
    let publication = if mode == InstallMode::Refresh && files.file_exists(Path::new(&path))? {
        scope.write_atomic(Path::new(&path), &contents)?;
        SkillPublication::Refreshed
    } else if scope.write_new(Path::new(&path), &contents)? {
        SkillPublication::Created
    } else {
        SkillPublication::Preserved
    };
    Ok(SkillPublicationFact { path, publication })
}

/// An installed skill whose marker differs from the shipped template's.
fn is_outdated(files: &RepositoryFiles, skill: &SkillTemplate) -> bool {
    files
        .read_optional_string(Path::new(&skill.agent_path()))
        .ok()
        .flatten()
        .is_some_and(|contents| skill_marker(&contents) != Some(skill.identity()))
}

#[derive(Debug)]
struct SkillTemplate {
    name: &'static str,
    contents: &'static str,
}

/// One shipped skill, read from `assets/skills/<name>/SKILL.md`.
macro_rules! skill {
    ($name:literal) => {
        SkillTemplate {
            name: $name,
            contents: include_str!(concat!("../../assets/skills/", $name, "/SKILL.md")),
        }
    };
}

const SKILLS: &[SkillTemplate] = &[
    skill!("criv"),
    skill!("criv-me"),
    skill!("writing-decisions"),
    skill!("referencing-code"),
    skill!("checking-drift"),
];

impl SkillTemplate {
    fn agent_path(&self) -> String {
        format!("{AGENT_SKILLS_DIR}/{}/SKILL.md", self.name)
    }

    fn claude_path(&self) -> String {
        format!("{CLAUDE_SKILLS_DIR}/{}/SKILL.md", self.name)
    }

    fn identity(&self) -> String {
        format!("blake3:{}", template_hash(&unstamp_skill(self.contents)))
    }
}

fn template_hash(contents: &str) -> String {
    blake3::hash(normalize_newlines(contents).as_bytes())
        .to_hex()
        .chars()
        .take(16)
        .collect()
}

fn metadata_children_end(lines: &[String], metadata: usize) -> usize {
    let child_start = metadata.saturating_add(1);
    lines.get(child_start..).map_or(lines.len(), |children| {
        children
            .iter()
            .position(|line| !line.starts_with(' ') && !line.starts_with('\t'))
            .and_then(|offset| child_start.checked_add(offset))
            .unwrap_or(lines.len())
    })
}

fn normalize_newlines(contents: &str) -> String {
    contents.replace("\r\n", "\n")
}

fn stamp_skill(contents: &str) -> String {
    let contents = unstamp_skill(&normalize_newlines(contents));
    let marker = format!("criv-template: blake3:{}", template_hash(&contents));
    let Some(frontmatter) = markdown::split_frontmatter(&contents) else {
        return contents;
    };
    let body = contents.get(frontmatter.body_start..).unwrap_or_default();

    let mut lines: Vec<String> = frontmatter.yaml.lines().map(str::to_string).collect();
    if let Some(index) = lines
        .iter()
        .position(|line| line.trim_start() == "metadata:")
    {
        let child_start = index.saturating_add(1);
        let insert_at = metadata_children_end(&lines, index);
        let marker_line = (child_start..insert_at).find(|&line| {
            lines
                .get(line)
                .is_some_and(|line| line.trim_start().starts_with("criv-template:"))
        });
        if let Some(marker_line) = marker_line {
            if let Some(line) = lines.get_mut(marker_line) {
                *line = format!("  {marker}");
            }
        } else {
            lines.insert(child_start, format!("  {marker}"));
        }
    } else {
        lines.push("metadata:".to_string());
        lines.push(format!("  {marker}"));
    }

    format!("---\n{}\n---\n{}", lines.join("\n"), body)
}

fn unstamp_skill(contents: &str) -> String {
    let normalized = normalize_newlines(contents);
    let contents = normalized.as_str();
    let Some(frontmatter) = markdown::split_frontmatter(contents) else {
        return contents.to_string();
    };
    let body = contents.get(frontmatter.body_start..).unwrap_or_default();

    let mut lines: Vec<String> = frontmatter.yaml.lines().map(str::to_string).collect();
    let Some(metadata) = lines
        .iter()
        .position(|line| line.trim_start() == "metadata:")
    else {
        return contents.to_string();
    };
    let child_start = metadata.saturating_add(1);
    let end = metadata_children_end(&lines, metadata);
    let marker_lines: Vec<usize> = (child_start..end)
        .filter(|&index| {
            lines
                .get(index)
                .is_some_and(|line| line.trim_start().starts_with("criv-template:"))
        })
        .collect();
    let has_metadata_child = (child_start..end).any(|index| {
        lines
            .get(index)
            .is_some_and(|line| !line.trim_start().starts_with("criv-template:"))
    });
    for index in marker_lines.into_iter().rev() {
        lines.remove(index);
    }
    if !has_metadata_child {
        lines.remove(metadata);
    }

    format!("---\n{}\n---\n{}", lines.join("\n"), body)
}

fn skill_marker(contents: &str) -> Option<String> {
    let mut in_metadata = false;
    for line in markdown::split_frontmatter(contents)?.yaml.lines() {
        if line.trim_start() == "metadata:" {
            in_metadata = true;
            continue;
        }
        if !line.starts_with(' ') && !line.starts_with('\t') {
            in_metadata = false;
        }
        if in_metadata && let Some(value) = line.trim().strip_prefix("criv-template:") {
            return Some(value.trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn install(root: &Path, mode: InstallMode) -> Result<InstallReport> {
        install_skills(&RepositoryFiles::open(root)?, mode)
    }

    fn outdated_at(root: &Path) -> Vec<String> {
        outdated_paths(&RepositoryFiles::open(root).unwrap())
    }

    fn claude_layout(root: &Path) -> LinkLayout {
        RepositoryFiles::open(root)
            .unwrap()
            .link_layout(Path::new(CLAUDE_SKILLS_DIR), Path::new(AGENT_SKILLS_DIR))
            .unwrap()
    }

    #[test]
    fn shipped_table_lists_every_skill_folder() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/skills");
        let mut folders: Vec<String> = fs::read_dir(assets)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        folders.sort();
        let mut table: Vec<&str> = SKILLS.iter().map(|skill| skill.name).collect();
        table.sort_unstable();
        assert_eq!(folders, table);
    }

    #[test]
    fn outdated_paths_name_stale_and_unmarked_skills_and_a_copied_claude_directory() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        assert!(
            outdated_at(root).is_empty(),
            "missing skills are not outdated"
        );

        install(root, InstallMode::CreateOnly).unwrap();
        assert!(outdated_at(root).is_empty());
        let stale_path = SKILLS[0].agent_path();
        let unmarked_path = SKILLS[1].agent_path();
        let stale = fs::read_to_string(root.join(&stale_path)).unwrap();
        fs::write(
            root.join(&stale_path),
            stale.replace("criv-template: blake3:", "criv-template: blake3:stale-"),
        )
        .unwrap();
        fs::write(root.join(&unmarked_path), "legacy skill\n").unwrap();
        fs::write(root.join(SKILLS[2].agent_path()), [0xff, 0xfe]).unwrap();
        assert_eq!(
            outdated_at(root),
            [stale_path.clone(), unmarked_path.clone()]
        );

        let claude = root.join(CLAUDE_SKILLS_DIR);
        remove_link_or_dir(&claude);
        fs::create_dir_all(claude.join("criv")).unwrap();
        assert_eq!(
            outdated_at(root),
            [CLAUDE_SKILLS_DIR.to_owned(), stale_path, unmarked_path]
        );
    }

    #[test]
    fn create_only_preserves_local_content_and_refresh_republishes_it() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let created = install(root, InstallMode::CreateOnly).unwrap();
        assert!(
            created
                .skills
                .iter()
                .all(|fact| fact.publication == SkillPublication::Created)
        );
        assert_eq!(created.claude, ClaudePublication::Linked);

        let path = SKILLS[0].agent_path();
        fs::write(root.join(&path), "local content\n").unwrap();
        let preserved = install(root, InstallMode::CreateOnly).unwrap();
        assert_eq!(preserved.skills[0].publication, SkillPublication::Preserved);
        assert_eq!(
            fs::read_to_string(root.join(&path)).unwrap(),
            "local content\n"
        );
        assert_eq!(preserved.claude, ClaudePublication::Current);

        let refreshed = install(root, InstallMode::Refresh).unwrap();
        assert!(
            refreshed
                .skills
                .iter()
                .all(|fact| fact.publication == SkillPublication::Refreshed)
        );
        assert!(outdated_at(root).is_empty());
    }

    #[test]
    fn copied_layout_is_blocked_until_explicit_refresh_replaces_it() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        install(root, InstallMode::CreateOnly).unwrap();
        let claude = root.join(CLAUDE_SKILLS_DIR);
        remove_link_or_dir(&claude);
        fs::create_dir_all(claude.join("criv")).unwrap();
        fs::write(claude.join("criv/SKILL.md"), "local copy\n").unwrap();

        let blocked = install(root, InstallMode::CreateOnly).unwrap();
        assert_eq!(blocked.claude, ClaudePublication::Blocked);
        assert_eq!(claude_layout(root), LinkLayout::Directory);
        assert_eq!(
            fs::read_to_string(claude.join("criv/SKILL.md")).unwrap(),
            "local copy\n"
        );

        let replaced = install(root, InstallMode::Refresh).unwrap();
        assert_eq!(replaced.claude, ClaudePublication::Replaced);
        assert_eq!(claude_layout(root), LinkLayout::Expected);
    }

    #[cfg(unix)]
    #[test]
    fn link_to_another_target_is_repaired_in_create_only_mode() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        install(root, InstallMode::CreateOnly).unwrap();
        let claude = root.join(CLAUDE_SKILLS_DIR);
        fs::remove_file(&claude).unwrap();
        fs::create_dir_all(root.join("wrong-skills")).unwrap();
        std::os::unix::fs::symlink("../wrong-skills", &claude).unwrap();

        assert_eq!(claude_layout(root), LinkLayout::Other);
        let repaired = install(root, InstallMode::CreateOnly).unwrap();
        assert_eq!(repaired.claude, ClaudePublication::Linked);
        assert_eq!(claude_layout(root), LinkLayout::Expected);
    }

    #[cfg(unix)]
    #[test]
    fn refresh_keeps_a_link_that_resolves_to_the_skills_by_another_path() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        install(root, InstallMode::CreateOnly).unwrap();
        let claude = root.join(CLAUDE_SKILLS_DIR);
        fs::remove_file(&claude).unwrap();
        std::os::unix::fs::symlink(root.join(AGENT_SKILLS_DIR), &claude).unwrap();

        let refreshed = install(root, InstallMode::Refresh).unwrap();

        assert_eq!(refreshed.claude, ClaudePublication::Current);
        assert_eq!(claude_layout(root), LinkLayout::Expected);
        for skill in SKILLS {
            assert!(root.join(skill.agent_path()).is_file());
        }
    }

    /// A junction reads as a directory; refresh must not empty the skills
    /// through it.
    #[cfg(windows)]
    #[test]
    fn refresh_keeps_a_junction_to_the_skills() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        install(root, InstallMode::CreateOnly).unwrap();
        let claude = root.join(CLAUDE_SKILLS_DIR);
        remove_link_or_dir(&claude);
        junction::create(root.join(AGENT_SKILLS_DIR), &claude).unwrap();

        let refreshed = install(root, InstallMode::Refresh).unwrap();

        assert_eq!(refreshed.claude, ClaudePublication::Current);
        for skill in SKILLS {
            assert!(root.join(skill.agent_path()).is_file());
        }
    }

    #[cfg(unix)]
    #[test]
    fn linked_claude_parent_is_rejected_before_skill_publication() {
        let temp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let root = temp.path();
        std::os::unix::fs::symlink(outside.path(), root.join(".claude")).unwrap();

        let error = install(root, InstallMode::CreateOnly).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("`.claude` must be a real repository directory")
        );
        assert!(!root.join(AGENT_SKILLS_DIR).exists());
    }

    #[test]
    fn unsupported_link_platform_copies_the_same_inventory() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let files = RepositoryFiles::open(root).unwrap();
        let scope = files.write_scope(Path::new(".")).unwrap();
        copy_claude_skills(&files, &scope, InstallMode::CreateOnly).unwrap();

        for skill in SKILLS {
            assert_eq!(
                fs::read_to_string(root.join(skill.claude_path())).unwrap(),
                stamp_skill(skill.contents)
            );
        }
        assert_eq!(claude_layout(root), LinkLayout::Directory);
    }

    #[test]
    fn canonical_identity_handles_crlf_and_accidentally_stamped_templates() {
        let skill = "---\nname: example\ndescription: Example\n---\n\n# Example\n";
        let crlf = skill.replace('\n', "\r\n");
        assert_eq!(template_hash(skill), template_hash(&crlf));

        let stamped = stamp_skill(&crlf);
        let frontmatter = stamped
            .strip_prefix("---\n")
            .and_then(|value| value.split_once("---\n"))
            .map(|(frontmatter, _)| frontmatter)
            .unwrap();
        serde_norway::from_str::<serde_norway::Value>(frontmatter).unwrap();
        assert_eq!(stamp_skill(&stamped), stamped);
        assert_eq!(unstamp_skill(&stamped), skill);
    }

    fn remove_link_or_dir(path: &Path) {
        let metadata = fs::symlink_metadata(path).unwrap();
        if metadata.file_type().is_symlink() {
            #[cfg(unix)]
            fs::remove_file(path).unwrap();
            #[cfg(windows)]
            {
                if junction::exists(path).unwrap_or(false) {
                    junction::delete(path).unwrap();
                }
                let _ = fs::remove_dir(path);
            }
        } else {
            fs::remove_dir_all(path).unwrap();
        }
    }
}
