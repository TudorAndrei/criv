//! Lifecycle for the generated runtime skills shipped with criv.
//!
//! Callers install the canonical inventory or inspect its best-effort status.
//! Template identity, marker handling, confined publication, and the
//! `.claude/skills` link-or-copy policy stay inside this module.

use std::fs;
use std::path::Path;

use crate::Result;
use crate::util::{
    LinkOutcome, file_exists_in, link_dir_in, read_optional_to_string_in, write_atomic_in,
    write_new_in,
};

const AGENT_SKILLS_DIR: &str = ".agents/skills";
const CLAUDE_SKILLS_DIR: &str = ".claude/skills";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum InstallMode {
    CreateOnly,
    Refresh,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum SkillPublication {
    Created,
    Refreshed,
    Preserved,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct SkillPublicationFact {
    pub(crate) path: &'static str,
    pub(crate) publication: SkillPublication,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ClaudePublication {
    Current,
    Linked,
    Replaced,
    Copied,
    Blocked,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct InstallReport {
    skills: Vec<SkillPublicationFact>,
    claude: ClaudePublication,
}

impl InstallReport {
    pub(crate) fn skill_publications(&self) -> &[SkillPublicationFact] {
        &self.skills
    }

    pub(crate) fn claude_publication(&self) -> ClaudePublication {
        self.claude
    }
}

pub(crate) fn describe_claude_publication(publication: ClaudePublication) -> &'static str {
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
pub(crate) enum InstalledSkillStatus {
    Missing,
    Current,
    Stale,
    Legacy,
    Unreadable,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct InstalledSkillFact {
    path: &'static str,
    status: InstalledSkillStatus,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ClaudeLayout {
    Missing,
    Linked,
    Copied,
    Other,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct SkillInventory {
    skills: Vec<InstalledSkillFact>,
    claude: ClaudeLayout,
}

impl SkillInventory {
    fn skill_statuses(&self) -> &[InstalledSkillFact] {
        &self.skills
    }

    fn claude_layout(&self) -> ClaudeLayout {
        self.claude
    }

    /// Paths that preserve the text-only stale-skill advisory contract.
    /// Missing and unreadable installations remain deliberately best-effort.
    pub(crate) fn advisory_outdated_paths(&self) -> Vec<&'static str> {
        let mut outdated = Vec::new();
        if self.claude_layout() == ClaudeLayout::Copied {
            outdated.push(CLAUDE_SKILLS_DIR);
        }
        outdated.extend(self.skill_statuses().iter().filter_map(|fact| {
            matches!(
                fact.status,
                InstalledSkillStatus::Stale | InstalledSkillStatus::Legacy
            )
            .then_some(fact.path)
        }));
        outdated
    }

    #[cfg(test)]
    pub(crate) fn status(&self, path: &str) -> Option<InstalledSkillStatus> {
        self.skills
            .iter()
            .find(|fact| fact.path == path)
            .map(|fact| fact.status)
    }
}

/// Install all generated skills through one create-only or refresh operation.
pub(crate) fn install(root: &Path, mode: InstallMode) -> Result<InstallReport> {
    install_with_linker(root, mode, |root, replace_directory| {
        link_dir_in(
            root,
            Path::new(CLAUDE_SKILLS_DIR),
            Path::new(AGENT_SKILLS_DIR),
            replace_directory,
        )
    })
}

/// Inspect installed skill identity without making advisory checks fallible.
pub(crate) fn inventory(root: &Path) -> SkillInventory {
    let skills = SKILLS
        .iter()
        .map(|skill| InstalledSkillFact {
            path: skill.agent_path,
            status: installed_status(root, skill),
        })
        .collect();
    SkillInventory {
        skills,
        claude: claude_layout(root),
    }
}

fn install_with_linker(
    root: &Path,
    mode: InstallMode,
    linker: impl FnOnce(&Path, bool) -> Result<LinkOutcome>,
) -> Result<InstallReport> {
    let mut skills = Vec::with_capacity(SKILLS.len());
    for skill in SKILLS {
        skills.push(publish_skill(root, skill.agent_path, skill.contents, mode)?);
    }

    let replace = mode == InstallMode::Refresh;
    let link_outcome = if claude_layout(root) == ClaudeLayout::Linked {
        LinkOutcome::Unchanged
    } else {
        linker(root, replace)?
    };
    let claude = match link_outcome {
        LinkOutcome::Unchanged => ClaudePublication::Current,
        LinkOutcome::Created => ClaudePublication::Linked,
        LinkOutcome::Replaced => ClaudePublication::Replaced,
        LinkOutcome::DirectoryInTheWay => ClaudePublication::Blocked,
        LinkOutcome::Unsupported => {
            for skill in SKILLS {
                publish_skill(root, skill.claude_path, skill.contents, mode)?;
            }
            ClaudePublication::Copied
        }
    };

    Ok(InstallReport { skills, claude })
}

fn publish_skill(
    root: &Path,
    path: &'static str,
    contents: &str,
    mode: InstallMode,
) -> Result<SkillPublicationFact> {
    let contents = stamp_skill(contents);
    let publication = if mode == InstallMode::Refresh && file_exists_in(root, Path::new(path))? {
        write_atomic_in(root, Path::new("."), Path::new(path), &contents)?;
        SkillPublication::Refreshed
    } else if write_new_in(root, Path::new("."), Path::new(path), &contents)? {
        SkillPublication::Created
    } else {
        SkillPublication::Preserved
    };
    Ok(SkillPublicationFact { path, publication })
}

fn installed_status(root: &Path, skill: &SkillTemplate) -> InstalledSkillStatus {
    let contents = match read_optional_to_string_in(root, Path::new(skill.agent_path)) {
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

fn claude_layout(root: &Path) -> ClaudeLayout {
    let path = root.join(CLAUDE_SKILLS_DIR);
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return ClaudeLayout::Missing;
    };
    let has_canonical_target = fs::canonicalize(&path)
        .ok()
        .zip(fs::canonicalize(root.join(AGENT_SKILLS_DIR)).ok())
        .is_some_and(|(link, target)| link == target);
    let has_expected_relative_target = metadata.file_type().is_symlink()
        && fs::read_link(&path).is_ok_and(|target| target == Path::new("../.agents/skills"));
    if has_canonical_target || has_expected_relative_target {
        ClaudeLayout::Linked
    } else if metadata.is_dir() {
        ClaudeLayout::Copied
    } else {
        ClaudeLayout::Other
    }
}

#[derive(Debug)]
struct SkillTemplate {
    agent_path: &'static str,
    claude_path: &'static str,
    contents: &'static str,
}

impl SkillTemplate {
    fn identity(&self) -> String {
        format!("blake3:{}", template_hash(&unstamp_skill(self.contents)))
    }
}

fn template_hash(contents: &str) -> String {
    blake3::hash(normalize_newlines(contents).as_bytes()).to_hex()[..16].to_string()
}

fn normalize_newlines(contents: &str) -> String {
    contents.replace("\r\n", "\n")
}

fn stamp_skill(contents: &str) -> String {
    let contents = unstamp_skill(&normalize_newlines(contents));
    let marker = format!("criv-template: blake3:{}", template_hash(&contents));
    let Some(rest) = contents.strip_prefix("---\n") else {
        return contents.to_string();
    };
    let Some((frontmatter, body)) = rest.split_once("---\n") else {
        return contents.to_string();
    };

    let mut lines: Vec<String> = frontmatter.lines().map(str::to_string).collect();
    if let Some(index) = lines
        .iter()
        .position(|line| line.trim_start() == "metadata:")
    {
        let insert_at = lines[index + 1..]
            .iter()
            .position(|line| !line.starts_with(' ') && !line.starts_with('\t'))
            .map_or(lines.len(), |offset| index + 1 + offset);
        let marker_line = (index + 1..insert_at)
            .find(|&line| lines[line].trim_start().starts_with("criv-template:"));
        if let Some(marker_line) = marker_line {
            lines[marker_line] = format!("  {marker}");
        } else {
            lines.insert(index + 1, format!("  {marker}"));
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
    let end = lines[metadata + 1..]
        .iter()
        .position(|line| !line.starts_with(' ') && !line.starts_with('\t'))
        .map_or(lines.len(), |offset| metadata + 1 + offset);
    let marker_lines: Vec<usize> = (metadata + 1..end)
        .filter(|&index| lines[index].trim_start().starts_with("criv-template:"))
        .collect();
    let has_metadata_child =
        (metadata + 1..end).any(|index| !lines[index].trim_start().starts_with("criv-template:"));
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

const SKILL_CRIV: &str = include_str!("../../assets/skills/criv/SKILL.md");
const SKILL_CRIV_ME: &str = include_str!("../../assets/skills/criv-me/SKILL.md");
const SKILL_WRITING_DECISIONS: &str =
    include_str!("../../assets/skills/writing-decisions/SKILL.md");
const SKILL_REFERENCING_CODE: &str = include_str!("../../assets/skills/referencing-code/SKILL.md");
const SKILL_CHECKING_DRIFT: &str = include_str!("../../assets/skills/checking-drift/SKILL.md");
const SKILL_C4_AUTHORING: &str = include_str!("../../assets/skills/c4-authoring/SKILL.md");

const SKILLS: &[SkillTemplate] = &[
    SkillTemplate {
        agent_path: ".agents/skills/criv/SKILL.md",
        claude_path: ".claude/skills/criv/SKILL.md",
        contents: SKILL_CRIV,
    },
    SkillTemplate {
        agent_path: ".agents/skills/criv-me/SKILL.md",
        claude_path: ".claude/skills/criv-me/SKILL.md",
        contents: SKILL_CRIV_ME,
    },
    SkillTemplate {
        agent_path: ".agents/skills/writing-decisions/SKILL.md",
        claude_path: ".claude/skills/writing-decisions/SKILL.md",
        contents: SKILL_WRITING_DECISIONS,
    },
    SkillTemplate {
        agent_path: ".agents/skills/referencing-code/SKILL.md",
        claude_path: ".claude/skills/referencing-code/SKILL.md",
        contents: SKILL_REFERENCING_CODE,
    },
    SkillTemplate {
        agent_path: ".agents/skills/checking-drift/SKILL.md",
        claude_path: ".claude/skills/checking-drift/SKILL.md",
        contents: SKILL_CHECKING_DRIFT,
    },
    SkillTemplate {
        agent_path: ".agents/skills/c4-authoring/SKILL.md",
        claude_path: ".claude/skills/c4-authoring/SKILL.md",
        contents: SKILL_C4_AUTHORING,
    },
];

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn inventory_distinguishes_missing_current_stale_legacy_and_unreadable() {
        let root = temp_root("inventory-status");
        let missing = inventory(&root);
        assert!(
            SKILLS.iter().all(
                |skill| missing.status(skill.agent_path) == Some(InstalledSkillStatus::Missing)
            )
        );
        assert_eq!(missing.claude, ClaudeLayout::Missing);

        install(&root, InstallMode::CreateOnly).unwrap();
        let stale_path = SKILLS[0].agent_path;
        let legacy_path = SKILLS[1].agent_path;
        let unreadable_path = SKILLS[2].agent_path;
        let current_path = SKILLS[3].agent_path;
        let stale = fs::read_to_string(root.join(stale_path)).unwrap();
        fs::write(
            root.join(stale_path),
            stale.replace("criv-template: blake3:", "criv-template: blake3:stale-"),
        )
        .unwrap();
        fs::write(root.join(legacy_path), "legacy skill\n").unwrap();
        fs::write(root.join(unreadable_path), [0xff, 0xfe]).unwrap();

        let statuses = inventory(&root);
        assert_eq!(
            statuses.status(stale_path),
            Some(InstalledSkillStatus::Stale)
        );
        assert_eq!(
            statuses.status(legacy_path),
            Some(InstalledSkillStatus::Legacy)
        );
        assert_eq!(
            statuses.status(unreadable_path),
            Some(InstalledSkillStatus::Unreadable)
        );
        assert_eq!(
            statuses.status(current_path),
            Some(InstalledSkillStatus::Current)
        );
        assert_eq!(statuses.claude, ClaudeLayout::Linked);
        assert_eq!(
            statuses.advisory_outdated_paths(),
            vec![stale_path, legacy_path]
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

        let path = SKILLS[0].agent_path;
        fs::write(root.join(path), "local content\n").unwrap();
        let preserved = install(&root, InstallMode::CreateOnly).unwrap();
        assert_eq!(preserved.skills[0].publication, SkillPublication::Preserved);
        assert_eq!(
            fs::read_to_string(root.join(path)).unwrap(),
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
            inventory(&root).status(path),
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
        assert_eq!(inventory(&root).claude, ClaudeLayout::Copied);
        assert_eq!(
            fs::read_to_string(claude.join("criv/SKILL.md")).unwrap(),
            "local copy\n"
        );

        let replaced = install(&root, InstallMode::Refresh).unwrap();
        assert_eq!(replaced.claude, ClaudePublication::Replaced);
        assert_eq!(inventory(&root).claude, ClaudeLayout::Linked);

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

        assert_eq!(inventory(&root).claude, ClaudeLayout::Other);
        let repaired = install(&root, InstallMode::CreateOnly).unwrap();
        assert_eq!(repaired.claude, ClaudePublication::Linked);
        assert_eq!(inventory(&root).claude, ClaudeLayout::Linked);

        remove_root(root);
    }

    #[test]
    fn unsupported_link_platform_copies_the_same_inventory() {
        let root = temp_root("copy-fallback");
        let report = install_with_linker(&root, InstallMode::CreateOnly, |_, _| {
            Ok(LinkOutcome::Unsupported)
        })
        .unwrap();

        assert_eq!(report.claude, ClaudePublication::Copied);
        for skill in SKILLS {
            assert_eq!(
                fs::read_to_string(root.join(skill.agent_path)).unwrap(),
                fs::read_to_string(root.join(skill.claude_path)).unwrap()
            );
        }
        assert_eq!(inventory(&root).claude, ClaudeLayout::Copied);

        remove_root(root);
    }

    #[test]
    fn repository_installation_matches_the_shipped_inventory() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let status = inventory(root);
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
            crate::util::remove_dir_link(path).unwrap();
        } else {
            fs::remove_dir_all(path).unwrap();
        }
    }

    fn remove_root(root: PathBuf) {
        let _ = fs::remove_dir_all(root);
    }
}
