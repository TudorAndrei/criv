//! Installation interface for generated skills and the optional editor viewer.

mod editor;
mod skills;

pub use editor::{InstallEditorOptions, run as install_editor};
pub use skills::{
    InstallMode, InstallReport, SkillPublication, describe_claude_publication,
    install_from as install_skills_from, inventory_from as skill_inventory_from,
};

#[cfg(test)]
pub(crate) use skills::inventory as skill_inventory;

#[cfg(test)]
pub(crate) use skills::InstalledSkillStatus;
