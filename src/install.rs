//! Installation interface for generated skills and the optional editor viewer.

mod editor;
mod skills;

pub use editor::{InstallEditorOptions, install_editor};
pub use skills::{
    InstallMode, SkillPublication, describe_claude_publication, install_skills,
    outdated_skills_advisory,
};
