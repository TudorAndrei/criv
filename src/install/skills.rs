//! Lifecycle for the generated runtime skills shipped with criv.
//!
//! Callers install the canonical inventory or ask for the best-effort
//! outdated-skills advisory.
//! Template identity, marker handling, confined publication, and the
//! `.claude/skills` link-or-copy policy stay inside this module.

use std::path::Path;

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

    pub(crate) const fn claude_publication(&self) -> ClaudePublication {
        self.claude
    }
}

pub const fn describe_claude_publication(publication: ClaudePublication) -> &'static str {
    match publication {
        ClaudePublication::Current => "skipped .claude/skills: already linked to .agents/skills",
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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InstalledSkillStatus {
    Missing,
    Current,
    Stale,
    Legacy,
    Unreadable,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct InstalledSkillFact {
    path: String,
    status: InstalledSkillStatus,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ClaudeLayout {
    Missing,
    Linked,
    Copied,
    Other,
}

#[derive(Debug, Eq, PartialEq)]
pub struct SkillInventory {
    skills: Vec<InstalledSkillFact>,
    claude: ClaudeLayout,
}

impl SkillInventory {
    /// Paths that preserve the text-only stale-skill advisory contract.
    /// Missing and unreadable installations remain deliberately best-effort.
    fn advisory_outdated_paths(&self) -> Vec<&str> {
        let mut outdated = Vec::new();
        if self.claude == ClaudeLayout::Copied {
            outdated.push(CLAUDE_SKILLS_DIR);
        }
        outdated.extend(self.skills.iter().filter_map(|fact| {
            matches!(
                fact.status,
                InstalledSkillStatus::Stale | InstalledSkillStatus::Legacy
            )
            .then_some(fact.path.as_str())
        }));
        outdated
    }
}

/// The text note that tells a user to refresh outdated agent skills.
pub fn outdated_skills_advisory(files: &RepositoryFiles) -> Option<String> {
    let outdated = inventory(files).advisory_outdated_paths().len();
    let subject = match outdated {
        0 => return None,
        1 => "skill is",
        _ => "skills are",
    };
    Some(format!(
        "note: {outdated} agent {subject} out of date; run `criv init --force-skills`"
    ))
}

fn inventory(files: &RepositoryFiles) -> SkillInventory {
    let skills = SKILLS
        .iter()
        .map(|skill| InstalledSkillFact {
            path: skill.agent_path(),
            status: installed_status(files, skill),
        })
        .collect();
    SkillInventory {
        skills,
        claude: claude_layout(files).unwrap_or(ClaudeLayout::Other),
    }
}

pub fn install_skills(files: &RepositoryFiles, mode: InstallMode) -> Result<InstallReport> {
    let initial_claude_layout = claude_layout(files).map_err(|error| {
        CrivError::new(format!(
            "cannot install Claude skills because `.claude` must be a real repository directory: {error}"
        ))
    })?;
    let scope = files.write_scope(Path::new("."))?;
    let skills = SKILLS
        .iter()
        .map(|skill| publish_skill(files, &scope, skill.agent_path(), skill.contents, mode))
        .collect::<Result<Vec<_>>>()?;

    let link_outcome = if initial_claude_layout == ClaudeLayout::Linked {
        LinkOutcome::Unchanged
    } else {
        scope.link_dir(
            Path::new(CLAUDE_SKILLS_DIR),
            Path::new(AGENT_SKILLS_DIR),
            mode == InstallMode::Refresh,
        )?
    };
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

fn installed_status(files: &RepositoryFiles, skill: &SkillTemplate) -> InstalledSkillStatus {
    let contents = match files.read_optional_string(Path::new(&skill.agent_path())) {
        Ok(Some(contents)) => contents,
        Ok(None) => return InstalledSkillStatus::Missing,
        Err(_) => return InstalledSkillStatus::Unreadable,
    };
    match skill_marker(&contents) {
        Some(marker) if marker == skill.identity() => InstalledSkillStatus::Current,
        Some(_) => InstalledSkillStatus::Stale,
        None => InstalledSkillStatus::Legacy,
    }
}

fn claude_layout(files: &RepositoryFiles) -> Result<ClaudeLayout> {
    let layout = files.link_layout(Path::new(CLAUDE_SKILLS_DIR), Path::new(AGENT_SKILLS_DIR))?;
    Ok(match layout {
        LinkLayout::Missing => ClaudeLayout::Missing,
        LinkLayout::Expected => ClaudeLayout::Linked,
        LinkLayout::Directory => ClaudeLayout::Copied,
        LinkLayout::Other => ClaudeLayout::Other,
    })
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
    let Some(rest) = contents.strip_prefix("---\n") else {
        return contents;
    };
    let Some((frontmatter, body)) = rest.split_once("---\n") else {
        return contents.clone();
    };

    let mut lines: Vec<String> = frontmatter.lines().map(str::to_string).collect();
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
    let Some(rest) = contents.strip_prefix("---\n") else {
        return contents.to_string();
    };
    let Some((frontmatter, body)) = rest.split_once("---\n") else {
        return contents.to_string();
    };

    let mut lines: Vec<String> = frontmatter.lines().map(str::to_string).collect();
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
    let normalized = normalize_newlines(contents);
    let rest = normalized.strip_prefix("---\n")?;
    let (frontmatter, _) = rest.split_once("---\n")?;
    let mut in_metadata = false;
    for line in frontmatter.lines() {
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
    use std::path::PathBuf;

    use super::*;

    fn install(root: &Path, mode: InstallMode) -> Result<InstallReport> {
        install_skills(&RepositoryFiles::open(root)?, mode)
    }

    fn inventory_at(root: &Path) -> SkillInventory {
        inventory(&RepositoryFiles::open(root).unwrap())
    }

    fn status(inventory: &SkillInventory, path: &str) -> Option<InstalledSkillStatus> {
        inventory
            .skills
            .iter()
            .find(|fact| fact.path == path)
            .map(|fact| fact.status)
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
    fn inventory_distinguishes_missing_current_stale_legacy_and_unreadable() {
        let root = temp_root("inventory-status");
        let missing = inventory_at(&root);
        assert!(SKILLS.iter().all(|skill| {
            status(&missing, &skill.agent_path()) == Some(InstalledSkillStatus::Missing)
        }));
        assert_eq!(missing.claude, ClaudeLayout::Missing);

        install(&root, InstallMode::CreateOnly).unwrap();
        let stale_path = SKILLS[0].agent_path();
        let legacy_path = SKILLS[1].agent_path();
        let unreadable_path = SKILLS[2].agent_path();
        let current_path = SKILLS[3].agent_path();
        let stale = fs::read_to_string(root.join(&stale_path)).unwrap();
        fs::write(
            root.join(&stale_path),
            stale.replace("criv-template: blake3:", "criv-template: blake3:stale-"),
        )
        .unwrap();
        fs::write(root.join(&legacy_path), "legacy skill\n").unwrap();
        fs::write(root.join(&unreadable_path), [0xff, 0xfe]).unwrap();

        let statuses = inventory_at(&root);
        assert_eq!(
            status(&statuses, &stale_path),
            Some(InstalledSkillStatus::Stale)
        );
        assert_eq!(
            status(&statuses, &legacy_path),
            Some(InstalledSkillStatus::Legacy)
        );
        assert_eq!(
            status(&statuses, &unreadable_path),
            Some(InstalledSkillStatus::Unreadable)
        );
        assert_eq!(
            status(&statuses, &current_path),
            Some(InstalledSkillStatus::Current)
        );
        assert_eq!(statuses.claude, ClaudeLayout::Linked);
        assert_eq!(
            statuses.advisory_outdated_paths(),
            vec![stale_path.as_str(), legacy_path.as_str()]
        );

        remove_root(root);
    }

    #[test]
    fn create_only_preserves_local_content_and_refresh_republishes_it() {
        let root = temp_root("publication-modes");
        let created = install(&root, InstallMode::CreateOnly).unwrap();
        assert!(
            created
                .skills
                .iter()
                .all(|fact| fact.publication == SkillPublication::Created)
        );
        assert_eq!(created.claude, ClaudePublication::Linked);

        let path = SKILLS[0].agent_path();
        fs::write(root.join(&path), "local content\n").unwrap();
        let preserved = install(&root, InstallMode::CreateOnly).unwrap();
        assert_eq!(preserved.skills[0].publication, SkillPublication::Preserved);
        assert_eq!(
            fs::read_to_string(root.join(&path)).unwrap(),
            "local content\n"
        );
        assert_eq!(preserved.claude, ClaudePublication::Current);

        let refreshed = install(&root, InstallMode::Refresh).unwrap();
        assert!(
            refreshed
                .skills
                .iter()
                .all(|fact| fact.publication == SkillPublication::Refreshed)
        );
        assert_eq!(
            status(&inventory_at(&root), &path),
            Some(InstalledSkillStatus::Current)
        );

        remove_root(root);
    }

    #[test]
    fn copied_layout_is_blocked_until_explicit_refresh_replaces_it() {
        let root = temp_root("copied-layout");
        install(&root, InstallMode::CreateOnly).unwrap();
        let claude = root.join(CLAUDE_SKILLS_DIR);
        remove_link_or_dir(&claude);
        fs::create_dir_all(claude.join("criv")).unwrap();
        fs::write(claude.join("criv/SKILL.md"), "local copy\n").unwrap();

        let blocked = install(&root, InstallMode::CreateOnly).unwrap();
        assert_eq!(blocked.claude, ClaudePublication::Blocked);
        assert_eq!(inventory_at(&root).claude, ClaudeLayout::Copied);
        assert_eq!(
            fs::read_to_string(claude.join("criv/SKILL.md")).unwrap(),
            "local copy\n"
        );

        let replaced = install(&root, InstallMode::Refresh).unwrap();
        assert_eq!(replaced.claude, ClaudePublication::Replaced);
        assert_eq!(inventory_at(&root).claude, ClaudeLayout::Linked);

        remove_root(root);
    }

    #[cfg(unix)]
    #[test]
    fn link_to_another_target_is_repaired_in_create_only_mode() {
        let root = temp_root("wrong-link-target");
        install(&root, InstallMode::CreateOnly).unwrap();
        let claude = root.join(CLAUDE_SKILLS_DIR);
        fs::remove_file(&claude).unwrap();
        fs::create_dir_all(root.join("wrong-skills")).unwrap();
        std::os::unix::fs::symlink("../wrong-skills", &claude).unwrap();

        assert_eq!(inventory_at(&root).claude, ClaudeLayout::Other);
        let repaired = install(&root, InstallMode::CreateOnly).unwrap();
        assert_eq!(repaired.claude, ClaudePublication::Linked);
        assert_eq!(inventory_at(&root).claude, ClaudeLayout::Linked);

        remove_root(root);
    }

    #[cfg(unix)]
    #[test]
    fn linked_claude_parent_is_rejected_before_skill_publication() {
        let root = temp_root("linked-claude-parent");
        let outside = temp_root("linked-claude-parent-outside");
        std::os::unix::fs::symlink(&outside, root.join(".claude")).unwrap();

        let error = install(&root, InstallMode::CreateOnly).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("`.claude` must be a real repository directory")
        );
        assert!(!root.join(AGENT_SKILLS_DIR).exists());

        fs::remove_file(root.join(".claude")).unwrap();
        remove_root(root);
        remove_root(outside);
    }

    #[test]
    fn unsupported_link_platform_copies_the_same_inventory() {
        let root = temp_root("copy-fallback");
        let files = RepositoryFiles::open(&root).unwrap();
        let scope = files.write_scope(Path::new(".")).unwrap();
        copy_claude_skills(&files, &scope, InstallMode::CreateOnly).unwrap();

        for skill in SKILLS {
            assert_eq!(
                fs::read_to_string(root.join(skill.claude_path())).unwrap(),
                stamp_skill(skill.contents)
            );
        }
        assert_eq!(inventory_at(&root).claude, ClaudeLayout::Copied);

        remove_root(root);
    }

    #[test]
    fn repository_installation_matches_the_shipped_inventory() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let status = inventory_at(root);
        assert!(
            status
                .skills
                .iter()
                .all(|fact| { fact.status == InstalledSkillStatus::Current })
        );
        assert_eq!(status.claude, ClaudeLayout::Linked);
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

    fn temp_root(label: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("criv-generated-skills-{label}-{unique}"));
        fs::create_dir_all(&root).unwrap();
        root
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

    fn remove_root(root: PathBuf) {
        let _ = fs::remove_dir_all(root);
    }
}
