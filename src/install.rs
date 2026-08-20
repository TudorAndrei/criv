//! Installation interface for generated skills and the optional editor viewer.

mod editor;
mod skills;

pub(crate) use editor::{InstallEditorOptions, run as install_editor};
pub(crate) use skills::{
    InstallMode, InstallReport, SkillPublication, describe_claude_publication,
    install as install_skills, inventory as skill_inventory,
};

#[cfg(test)]
pub(crate) use skills::InstalledSkillStatus;
