//! C4 artifact and workspace loading interface.

mod artifact;
mod likec4;

pub(crate) use artifact::{C4Artifact, C4ArtifactFormat, parse_file};
pub(crate) use likec4::{LikeC4Workspace, load as load_workspace};
