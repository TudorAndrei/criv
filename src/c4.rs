use std::path::Path;

use crate::Result;
use crate::discovery::read_selected_text;
use crate::util::strip_prefix;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum C4ArtifactFormat {
    LikeC4,
}

#[derive(Debug, Clone)]
pub(crate) struct C4Artifact {
    pub(crate) path: std::path::PathBuf,
    pub(crate) rel_path: String,
    pub(crate) format: Option<C4ArtifactFormat>,
    pub(crate) diagnostics: Vec<C4ArtifactDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct C4ArtifactDiagnostic {
    pub(crate) code: &'static str,
    pub(crate) line: Option<usize>,
    pub(crate) message: String,
}

pub(crate) fn parse_file(root: &Path, docs_path: &Path, path: &Path) -> Result<C4Artifact> {
    let contents = read_selected_text(root, path)?;
    Ok(parse_contents(root, docs_path, path, &contents))
}

fn parse_contents(root: &Path, docs_path: &Path, path: &Path, contents: &str) -> C4Artifact {
    let rel_path = strip_prefix(path, root);
    let mut diagnostics = Vec::new();
    let directives = generated_directives(contents);
    let format = is_likec4(contents).then_some(C4ArtifactFormat::LikeC4);
    if format.is_none() {
        diagnostics.push(C4ArtifactDiagnostic {
            code: "unknown-c4-format",
            line: first_non_empty_line(contents),
            message: ".c4 content must use LikeC4 DSL".into(),
        });
    }
    for (line, value) in &directives {
        if value
            .as_deref()
            .is_some_and(|value| !matches!(value, "true" | "false"))
        {
            diagnostics.push(C4ArtifactDiagnostic {
                code: "invalid-c4-generated",
                line: Some(*line),
                message: "criv:generated must be true or false".into(),
            });
        }
    }
    let doc_rel_path = strip_prefix(path, docs_path);
    C4Artifact {
        path: path.to_path_buf(),
        rel_path: if rel_path.is_empty() {
            doc_rel_path
        } else {
            rel_path
        },
        format,
        diagnostics,
    }
}

fn is_likec4(contents: &str) -> bool {
    contents.lines().any(|line| {
        let line = line.trim_start();
        [
            "specification",
            "model",
            "views",
            "deployment",
            "global",
            "extend",
        ]
        .iter()
        .any(|keyword| {
            line.strip_prefix(keyword)
                .is_some_and(|rest| rest.trim_start().starts_with('{'))
        })
    })
}

fn generated_directives(contents: &str) -> Vec<(usize, Option<String>)> {
    contents
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let body = line.trim().strip_prefix("//")?.trim();
            let value = body.strip_prefix("criv:generated")?.trim();
            Some((index + 1, (!value.is_empty()).then(|| value.to_string())))
        })
        .collect()
}

fn first_non_empty_line(contents: &str) -> Option<usize> {
    contents
        .lines()
        .position(|line| !line.trim().is_empty())
        .map(|index| index + 1)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn accepts_likec4_and_rejects_legacy_formats() {
        let root = Path::new("/repo");
        let docs = root.join("docs");
        let path = docs.join("architecture/model.c4");
        let likec4 = parse_contents(
            root,
            &docs,
            &path,
            "specification { element system }\nmodel { app = system 'App' }\n",
        );
        assert_eq!(likec4.format, Some(C4ArtifactFormat::LikeC4));
        assert!(likec4.diagnostics.is_empty());

        for legacy in ["C4Context\n", "digraph architecture { a -> b }\n"] {
            let artifact = parse_contents(root, &docs, &path, legacy);
            assert_eq!(artifact.format, None);
            assert_eq!(artifact.diagnostics[0].code, "unknown-c4-format");
        }
    }
}
