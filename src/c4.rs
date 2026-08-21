//! C4 artifact and workspace loading interface.

mod artifact;
mod likec4;

pub(crate) use artifact::{C4Artifact, C4ArtifactFormat, parse_file_from};
pub(crate) use likec4::LikeC4Workspace;

use std::path::Path;

pub(crate) fn load_workspace(
    root: &Path,
    docs_path: &Path,
    artifacts: &[C4Artifact],
) -> LikeC4Workspace {
    let sources = artifacts
        .iter()
        .filter(|artifact| artifact.format == Some(C4ArtifactFormat::LikeC4))
        .map(|artifact| likec4::LikeC4Source {
            path: artifact.path.clone(),
            source: artifact.source.clone(),
        })
        .collect::<Vec<_>>();
    likec4::load(root, docs_path, &sources)
}
